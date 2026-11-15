use anyhow::{anyhow, Result};
use rusqlite::{types::ToSql, Connection};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::embed;

#[cfg(test)]
pub(super) fn build_index(
    conn: &Connection,
    model_id: &str,
    dimensions: usize,
    capacity: usize,
) -> Result<Index> {
    build_index_scoped(conn, model_id, dimensions, capacity, None)
}

pub(super) fn build_index_scoped(
    conn: &Connection,
    model_id: &str,
    dimensions: usize,
    capacity: usize,
    source_id: Option<i64>,
) -> Result<Index> {
    let options = IndexOptions {
        dimensions,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        connectivity: 16,
        expansion_add: 128,
        expansion_search: 1_024,
        multi: false,
    };
    let index =
        Index::new(&options).map_err(|error| anyhow!("failed to create HNSW index: {error}"))?;
    index
        .reserve(capacity)
        .map_err(|error| anyhow!("failed to reserve HNSW index capacity: {error}"))?;

    let sql = match source_id {
        Some(_) => {
            "SELECT ke.chunk_rowid, ke.embedding, ke.dim
                    FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_source
                    CROSS JOIN knowledge_embeddings ke
                    WHERE kcm.source_id = ?2 AND ke.chunk_rowid = kcm.chunk_rowid
                      AND ke.model_id = ?1
                    ORDER BY kcm.chunk_rowid"
        }
        None => {
            "SELECT chunk_rowid, embedding, dim FROM knowledge_embeddings
                 WHERE model_id = ?1 ORDER BY chunk_rowid"
        }
    };
    let mut statement = conn.prepare(sql)?;
    let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(model_id.to_string())];
    if let Some(source_id) = source_id {
        params.push(Box::new(source_id));
    }
    let param_refs: Vec<&dyn ToSql> = params.iter().map(|param| param.as_ref()).collect();
    let rows = statement.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (rowid, bytes, stored_dimensions) = row?;
        if rowid <= 0 || stored_dimensions as usize != dimensions || bytes.len() != dimensions * 4 {
            anyhow::bail!("invalid embedding row {rowid} in model {model_id}");
        }
        let vector = embed::bytes_to_embedding(&bytes);
        if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
            anyhow::bail!("invalid vector contents for embedding row {rowid}");
        }
        index
            .add(rowid as u64, &vector)
            .map_err(|error| anyhow!("failed to add row {rowid} to HNSW index: {error}"))?;
    }
    Ok(index)
}
