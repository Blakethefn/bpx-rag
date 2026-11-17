use super::lexical::{
    chunk_columns, column_at, ranked_result_from_row, select_list, BoundColumn, ChunkColumn,
    Selected,
};
use super::rank::RankedResult;
use crate::embed;
use crate::knowledge::filter::{file_scope_sql, predicates_to_sql, FilterPredicate};
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
    predicates: &[FilterPredicate],
    model_id: &str,
    candidate_rowids: Option<&[i64]>,
) -> Result<Vec<RankedResult>> {
    if candidate_rowids.is_some_and(|rowids| rowids.is_empty()) || limit == 0 {
        return Ok(Vec::new());
    }
    let columns = vector_columns("kc");
    let select = select_list(&columns);
    let embedding_at = column_at(&columns, Selected::Embedding);
    let (sql, params) = vector_query(select, predicates, model_id, candidate_rowids)?;
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
fn vector_query(
    select: String,
    predicates: &[FilterPredicate],
    model_id: &str,
    candidate_rowids: Option<&[i64]>,
) -> Result<(String, Vec<Box<dyn rusqlite::types::ToSql>>)> {
    if candidate_rowids.is_none() {
        if let Some(query) = file_first_query(&select, predicates, model_id)? {
            return Ok(query);
        }
    }
    let (meta_where, meta_params) = predicates_to_sql(predicates, 0);
    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = meta_params
        .into_iter()
        .map(|value| Box::new(value) as _)
        .collect();
    let model_param = params.len() + 1;
    let mut sql = format!(
        "SELECT {select}
         FROM knowledge_embeddings ke
         JOIN knowledge_chunks kc ON kc.rowid = ke.chunk_rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE 1=1{meta_clause} AND ke.model_id = ?{model_param}"
    );
    params.push(Box::new(model_id.to_string()));
    if let Some(rowids) = candidate_rowids {
        let placeholders = rowids
            .iter()
            .enumerate()
            .map(|(index, _)| format!("?{}", params.len() + index + 1))
            .collect::<Vec<_>>();
        sql.push_str(&format!(
            " AND ke.chunk_rowid IN ({})",
            placeholders.join(",")
        ));
        params.extend(rowids.iter().map(|rowid| Box::new(*rowid) as _));
    }

    Ok((sql, params))
}
fn file_first_query(
    select: &str,
    predicates: &[FilterPredicate],
    model_id: &str,
) -> Result<Option<(String, Vec<Box<dyn rusqlite::types::ToSql>>)>> {
    if predicates
        .iter()
        .any(|predicate| matches!(predicate, FilterPredicate::Source(_)))
    {
        return Ok(None);
    }
    let path_predicates: Vec<_> = predicates
        .iter()
        .filter(|predicate| {
            matches!(
                predicate,
                FilterPredicate::Directory(_) | FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect();
    if path_predicates.is_empty() {
        return Ok(None);
    }

    let residual_predicates: Vec<_> = predicates
        .iter()
        .filter(|predicate| {
            !matches!(
                predicate,
                FilterPredicate::Directory(_) | FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect();
    let (file_scope, file_params) = file_scope_sql(&path_predicates, 0)
        .ok_or_else(|| anyhow::anyhow!("path filters did not produce a file scope"))?;
    let (metadata_where, metadata_params) =
        predicates_to_sql(&residual_predicates, file_params.len());
    let metadata_clause = if metadata_where.is_empty() {
        String::new()
    } else {
        format!(" AND {metadata_where}")
    };
    let model_param = file_params.len() + metadata_params.len() + 1;
    let sql = format!(
        "SELECT {select}
         FROM knowledge_files kf INDEXED BY idx_files_rel_path
         CROSS JOIN knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_file
         CROSS JOIN knowledge_embeddings ke
         CROSS JOIN knowledge_chunks kc
         CROSS JOIN knowledge_sources ks
         WHERE {file_scope}
           AND kcm.file_id = kf.id
           AND ke.chunk_rowid = kcm.chunk_rowid
           AND kc.rowid = ke.chunk_rowid
           AND ks.id = kcm.source_id
           AND ke.model_id = ?{model_param}{metadata_clause}
         ORDER BY ke.chunk_rowid"
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = file_params
        .into_iter()
        .chain(metadata_params)
        .map(|value| Box::new(value) as _)
        .collect();
    params.push(Box::new(model_id.to_string()));
    Ok(Some((sql, params)))
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
