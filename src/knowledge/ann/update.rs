use anyhow::{anyhow, Result};
use rusqlite::{params, types::ToSql, Connection, OptionalExtension};
use usearch::Index;

use crate::embed;

pub(super) struct IncrementalChanges<'a> {
    pub(super) conn: &'a Connection,
    pub(super) index: &'a Index,
    pub(super) model_id: &'a str,
    pub(super) dimensions: usize,
    pub(super) from_generation: i64,
    pub(super) to_generation: i64,
    pub(super) previous_tombstones: usize,
    pub(super) expected_size: usize,
    pub(super) source_id: Option<i64>,
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
        source_id,
    } = changes;
    let (first_change, change_count): (Option<i64>, i64) = match source_id {
        Some(source_id) => conn.query_row(
            "SELECT MIN(change_id), COUNT(*) FROM knowledge_embedding_changes
             WHERE change_id > ?1 AND change_id <= ?2 AND source_id = ?3",
            params![from_generation, to_generation, source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
        None => conn.query_row(
            "SELECT MIN(change_id), COUNT(*) FROM knowledge_embedding_changes
             WHERE change_id > ?1 AND change_id <= ?2",
            params![from_generation, to_generation],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?,
    };
    if source_id.is_none() && (change_count == 0 || first_change != Some(from_generation + 1)) {
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

    let rowids = changed_rowids(conn, from_generation, to_generation, source_id)?;
    let mut tombstones = previous_tombstones;
    for rowid in rowids {
        if rowid <= 0 {
            return Ok(None);
        }
        tombstones += index
            .remove(rowid as u64)
            .map_err(|error| anyhow!("failed to remove row {rowid} from HNSW: {error}"))?;
        let current = current_embedding(conn, rowid, model_id, source_id)?;
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

fn changed_rowids(
    conn: &Connection,
    from_generation: i64,
    to_generation: i64,
    source_id: Option<i64>,
) -> Result<Vec<i64>> {
    let (sql, params): (&str, Vec<Box<dyn ToSql>>) = match source_id {
        Some(source_id) => (
            "SELECT DISTINCT chunk_rowid FROM knowledge_embedding_changes
             WHERE change_id > ?1 AND change_id <= ?2 AND source_id = ?3",
            vec![
                Box::new(from_generation),
                Box::new(to_generation),
                Box::new(source_id),
            ],
        ),
        None => (
            "SELECT DISTINCT chunk_rowid FROM knowledge_embedding_changes
             WHERE change_id > ?1 AND change_id <= ?2",
            vec![Box::new(from_generation), Box::new(to_generation)],
        ),
    };
    let param_refs: Vec<&dyn ToSql> = params.iter().map(|param| param.as_ref()).collect();
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(param_refs.as_slice(), |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn current_embedding(
    conn: &Connection,
    rowid: i64,
    model_id: &str,
    source_id: Option<i64>,
) -> Result<Option<(Vec<u8>, i64)>> {
    match source_id {
        Some(source_id) => conn
            .query_row(
                "SELECT ke.embedding, ke.dim FROM knowledge_embeddings ke
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                 WHERE ke.chunk_rowid = ?1 AND ke.model_id = ?2 AND kcm.source_id = ?3",
                params![rowid, model_id, source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into),
        None => conn
            .query_row(
                "SELECT embedding, dim FROM knowledge_embeddings
                 WHERE chunk_rowid = ?1 AND model_id = ?2",
                params![rowid, model_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into),
    }
}
