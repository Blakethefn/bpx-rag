#[cfg(test)]
use crate::embed;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use usearch::Index;

mod build;
mod candidates;
mod sidecar;
mod update;

#[cfg(test)]
use build::build_index;
use build::build_index_scoped;
#[cfg(test)]
use sidecar::{load_sidecar, save_sidecar};
use sidecar::{load_sidecar_scoped, save_sidecar_scoped};
use update::{apply_incremental_changes, IncrementalChanges};

pub(super) const ANN_MIN_VECTORS: usize = 4_096;
const ANN_MAX_CANDIDATES: usize = 8_192;
const _: () = assert!(ANN_MAX_CANDIDATES >= 256);
const ANN_CANDIDATE_MULTIPLIER: usize = 512;
const SMALL_FILTER_EXACT_LIMIT: usize = 512;

pub struct VectorIndex {
    index: Index,
    dimensions: usize,
}

impl VectorIndex {
    #[cfg(test)]
    pub fn load_or_build(
        conn: &Connection,
        sidecar_dir: Option<&Path>,
        model_id: &str,
        dimensions: usize,
        generation: i64,
        database_id: &str,
    ) -> Result<Option<Self>> {
        Self::load_or_build_scoped(
            conn,
            sidecar_dir,
            model_id,
            dimensions,
            generation,
            database_id,
            None,
        )
    }

    pub(super) fn load_or_build_scoped(
        conn: &Connection,
        sidecar_dir: Option<&Path>,
        model_id: &str,
        dimensions: usize,
        generation: i64,
        database_id: &str,
        source_id: Option<i64>,
    ) -> Result<Option<Self>> {
        let vector_count = active_vector_count_scoped(conn, model_id, source_id)?;
        if vector_count == 0 {
            return Ok(None);
        }
        if vector_count < ANN_MIN_VECTORS {
            return Ok(None);
        }

        if let Some(dir) = sidecar_dir {
            if let Some((index, manifest)) = load_sidecar_scoped(
                dir,
                model_id,
                dimensions,
                generation,
                database_id,
                source_id,
            )? {
                if manifest.generation == generation && index.size() == vector_count {
                    return Ok(Some(Self { index, dimensions }));
                }
                if manifest.generation < generation {
                    let changes = IncrementalChanges {
                        conn,
                        index: &index,
                        model_id,
                        dimensions,
                        from_generation: manifest.generation,
                        to_generation: generation,
                        previous_tombstones: manifest.tombstones,
                        expected_size: vector_count,
                        source_id,
                    };
                    if let Some(tombstones) = apply_incremental_changes(changes)? {
                        let index_size = index.size();
                        let compact_threshold = (index_size / 10).max(1_000);
                        let tombstones = if tombstones >= compact_threshold {
                            index.compact().map_err(|error| {
                                anyhow!("failed to compact HNSW index: {error}")
                            })?;
                            0
                        } else {
                            tombstones
                        };
                        save_sidecar_scoped(
                            dir,
                            &index,
                            model_id,
                            dimensions,
                            generation,
                            database_id,
                            tombstones,
                            source_id,
                        )?;
                        return Ok(Some(Self { index, dimensions }));
                    }
                }
            }
        }

        let index = build_index_scoped(conn, model_id, dimensions, vector_count, source_id)?;
        if let Some(dir) = sidecar_dir {
            save_sidecar_scoped(
                dir,
                &index,
                model_id,
                dimensions,
                generation,
                database_id,
                0,
                source_id,
            )?;
        }

        Ok(Some(Self { index, dimensions }))
    }

    pub fn memory_usage_bytes(&self) -> usize {
        self.index.memory_usage()
    }
}

pub(super) fn remove_source_sidecars(dir: &Path, source_id: i64) -> Result<()> {
    sidecar::remove_source(dir, source_id)
}

fn active_vector_count_scoped(
    conn: &Connection,
    model_id: &str,
    source_id: Option<i64>,
) -> Result<usize> {
    let count = match source_id {
        Some(source_id) => conn.query_row(
            "SELECT COUNT(*)
             FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_source
             CROSS JOIN knowledge_embeddings ke
             WHERE kcm.source_id = ?2 AND ke.chunk_rowid = kcm.chunk_rowid
               AND ke.model_id = ?1",
            params![model_id, source_id],
            |row| row.get::<_, i64>(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM knowledge_embeddings WHERE model_id = ?1",
            params![model_id],
            |row| row.get::<_, i64>(0),
        )?,
    };
    Ok(count.max(0) as usize)
}

pub(super) fn source_generation(conn: &Connection, source_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(change_id), 0)
         FROM knowledge_embedding_changes WHERE source_id = ?1",
        params![source_id],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;
