use super::*;
use crate::embed;
use anyhow::Result;
use rusqlite::Connection;

/// Vector similarity search over knowledge embeddings.
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

    // Metadata filter params
    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let mut sql = format!(
        "SELECT ke.chunk_rowid, ke.embedding,
                kcm.file_id, kcm.source_id,
                kc.title, kc.content,
                COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                COALESCE(kcm.metadata, '{{}}'),
                COALESCE(ks.label, '') as source_label,
                COALESCE(kf.rel_path, '') as file_path
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
            let chunk_rowid: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((
                blob,
                chunk_rowid,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)? as u32,
                row.get::<_, i64>(7)? as u32,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(
            |(blob, rowid, file_id, source_id, title, content, ls, le, meta, source, path)| {
                let embedding = decode_stored_embedding(rowid, &blob, query_embedding.len())?;
                let similarity = dot_product(query_embedding, &embedding);
                Ok((
                    similarity,
                    RankedResult {
                        rowid,
                        file_id,
                        source_id,
                        title,
                        content,
                        rank: 0.0,
                        source_label: source,
                        file_path: path,
                        line_start: ls,
                        line_end: le,
                        metadata: meta,
                    },
                ))
            },
        )
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

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Decode a stored embedding, rejecting a blob that is not `dimensions` f32 values.
/// A short blob would otherwise be truncated and scored against a partial vector.
fn decode_stored_embedding(chunk_rowid: i64, blob: &[u8], dimensions: usize) -> Result<Vec<f32>> {
    if blob.len() != dimensions * 4 {
        anyhow::bail!(
            "embedding for chunk {chunk_rowid} is {} bytes; expected {} for {dimensions} dimensions",
            blob.len(),
            dimensions * 4
        );
    }
    Ok(embed::bytes_to_embedding(blob))
}

/// Dot product for cosine similarity (vectors are L2-normalized at index time).
pub(super) fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
