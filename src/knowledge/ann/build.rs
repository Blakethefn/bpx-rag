use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::embed;

pub(super) fn build_index(
    conn: &Connection,
    model_id: &str,
    dimensions: usize,
    capacity: usize,
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

    let mut statement = conn.prepare(
        "SELECT chunk_rowid, embedding, dim
         FROM knowledge_embeddings
         WHERE model_id = ?1
         ORDER BY chunk_rowid",
    )?;
    let rows = statement.query_map(params![model_id], |row| {
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

pub(super) struct IncrementalChanges<'a> {
    pub(super) conn: &'a Connection,
    pub(super) index: &'a Index,
    pub(super) model_id: &'a str,
    pub(super) dimensions: usize,
    pub(super) from_generation: i64,
    pub(super) to_generation: i64,
    pub(super) previous_tombstones: usize,
    pub(super) expected_size: usize,
}

pub(super) fn apply_incremental_changes(changes: IncrementalChanges<'_>) -> Result<Option<usize>> {
    let IncrementalChanges {
        conn,
        index,
        model_id,
        dimensions,
        from_generation,
        to_generation,
        previous_tombstones,
        expected_size,
    } = changes;
    let (first_change, change_count): (Option<i64>, i64) = conn.query_row(
        "SELECT MIN(change_id), COUNT(*) FROM knowledge_embedding_changes
         WHERE change_id > ?1 AND change_id <= ?2",
        params![from_generation, to_generation],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if change_count == 0 || first_change != Some(from_generation + 1) {
        return Ok(None);
    }
    let full_rebuild_threshold = (expected_size / 4).max(2_048);
    if change_count as usize > full_rebuild_threshold {
        return Ok(None);
    }

    if index.capacity() < expected_size {
        index
            .reserve(expected_size)
            .map_err(|error| anyhow!("failed to grow HNSW capacity to {expected_size}: {error}"))?;
    }

    let rowids = {
        let mut statement = conn.prepare(
            "SELECT DISTINCT chunk_rowid FROM knowledge_embedding_changes
             WHERE change_id > ?1 AND change_id <= ?2",
        )?;
        let rows = statement
            .query_map(params![from_generation, to_generation], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let mut tombstones = previous_tombstones;
    for rowid in rowids {
        if rowid <= 0 {
            return Ok(None);
        }
        tombstones += index
            .remove(rowid as u64)
            .map_err(|error| anyhow!("failed to remove row {rowid} from HNSW: {error}"))?;
        let current: Option<(Vec<u8>, i64)> = conn
            .query_row(
                "SELECT embedding, dim FROM knowledge_embeddings
                 WHERE chunk_rowid = ?1 AND model_id = ?2",
                params![rowid, model_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((bytes, stored_dimension)) = current {
            if stored_dimension as usize != dimensions || bytes.len() != dimensions * 4 {
                return Ok(None);
            }
            let vector = embed::bytes_to_embedding(&bytes);
            if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
                return Ok(None);
            }
            index
                .add(rowid as u64, &vector)
                .map_err(|error| anyhow!("failed to update row {rowid} in HNSW: {error}"))?;
        }
    }
    if index.size() != expected_size {
        return Ok(None);
    }
    Ok(Some(tombstones))
}
