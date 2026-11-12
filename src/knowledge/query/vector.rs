use super::lexical::{
    chunk_columns, column_at, ranked_result_from_row, select_list, BoundColumn, ChunkColumn,
    Selected,
};
use super::rank::RankedResult;
use crate::embed;
use anyhow::Result;
use rusqlite::Connection;

#[cfg(test)]
mod tests;

fn vector_columns(chunk: &str) -> Vec<BoundColumn> {
    let mut columns = chunk_columns(chunk);
    let rowid_at = column_at(&columns, Selected::Chunk(ChunkColumn::Rowid));
    columns.insert(
        rowid_at + 1,
        BoundColumn {
            selected: Selected::Embedding,
            sql: "ke.embedding".to_string(),
        },
    );
    columns
}

pub(super) fn knowledge_vector_search(
    conn: &Connection,
    query_embedding: &[f32],
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
    model_id: &str,
    candidate_rowids: Option<&[i64]>,
) -> Result<Vec<RankedResult>> {
    if candidate_rowids.is_some_and(|rowids| rowids.is_empty()) || limit == 0 {
        return Ok(Vec::new());
    }
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let columns = vector_columns("kc");
    let select = select_list(&columns);
    let embedding_at = column_at(&columns, Selected::Embedding);
    let mut sql = format!(
        "SELECT {select}
         FROM knowledge_embeddings ke
         JOIN knowledge_chunks kc ON kc.rowid = ke.chunk_rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE 1=1{meta_clause}"
    );

    let model_param_index = params.len() + 1;
    sql.push_str(&format!(" AND ke.model_id = ?{model_param_index}"));
    params.push(Box::new(model_id.to_string()));

    if let Some(rowids) = candidate_rowids {
        if !rowids.is_empty() {
            let placeholders: Vec<String> = rowids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + i + 1))
                .collect();
            sql.push_str(&format!(
                " AND ke.chunk_rowid IN ({})",
                placeholders.join(",")
            ));
            for &rid in rowids {
                params.push(Box::new(rid));
            }
        }
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let mut scored: Vec<(f32, RankedResult)> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let blob: Vec<u8> = row.get(embedding_at)?;
            let result = ranked_result_from_row(row, &columns)?;
            Ok((blob, result))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(blob, result)| {
            let embedding = decode_stored_embedding(result.rowid, &blob, query_embedding.len())?;
            let similarity = dot_product(query_embedding, &embedding);
            Ok((similarity, result))
        })
        .collect::<Result<Vec<_>>>()?;

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit as usize);

    Ok(scored
        .into_iter()
        .enumerate()
        .map(|(rank, (_, mut result))| {
            result.rank = rank as f64;
            result
        })
        .collect())
}

fn decode_stored_embedding(chunk_rowid: i64, blob: &[u8], dimensions: usize) -> Result<Vec<f32>> {
    if blob.len() != dimensions * 4 {
        anyhow::bail!(
            "embedding for chunk {chunk_rowid} is {} bytes; expected {} for {dimensions} dimensions",
            blob.len(),
            dimensions * 4
        );
    }
    let embedding = embed::bytes_to_embedding(blob);
    if embedding.iter().any(|value| !value.is_finite()) {
        anyhow::bail!("embedding for chunk {chunk_rowid} has a non-finite value");
    }
    Ok(embedding)
}

pub(super) fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
