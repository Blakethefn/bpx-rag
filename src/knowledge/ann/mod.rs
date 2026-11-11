//! Rebuildable USearch HNSW sidecar for large active-model vector sets.
//!
//! SQLite remains the source of truth. This index is a derived candidate
//! generator; every returned row ID is rechecked and exactly re-ranked in SQL.

#[cfg(test)]
use crate::embed;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;
use usearch::Index;

mod build;
mod sidecar;

use build::{apply_incremental_changes, build_index};
use sidecar::{load_sidecar, save_sidecar};

const ANN_MIN_VECTORS: usize = 4_096;
const ANN_MAX_CANDIDATES: usize = 8_192;
const ANN_CANDIDATE_MULTIPLIER: usize = 512;
const SMALL_FILTER_EXACT_LIMIT: usize = 512;

pub struct VectorIndex {
    index: Index,
    dimensions: usize,
}

impl VectorIndex {
    pub fn load_or_build(
        conn: &Connection,
        sidecar_dir: Option<&Path>,
        model_id: &str,
        dimensions: usize,
        generation: i64,
        database_id: &str,
    ) -> Result<Option<Self>> {
        let vector_count = active_vector_count(conn, model_id)?;
        if vector_count == 0 {
            return Ok(None);
        }
        if vector_count < ANN_MIN_VECTORS {
            return Ok(None);
        }

        if let Some(dir) = sidecar_dir {
            if let Some((index, manifest)) =
                load_sidecar(dir, model_id, dimensions, generation, database_id)?
            {
                if manifest.generation == generation && index.size() == vector_count {
                    return Ok(Some(Self { index, dimensions }));
                }
                if manifest.generation < generation {
                    if let Some(tombstones) = apply_incremental_changes(
                        conn,
                        &index,
                        model_id,
                        dimensions,
                        manifest.generation,
                        generation,
                        manifest.tombstones,
                        vector_count,
                    )? {
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
                        save_sidecar(
                            dir,
                            &index,
                            model_id,
                            dimensions,
                            generation,
                            database_id,
                            tombstones,
                        )?;
                        return Ok(Some(Self { index, dimensions }));
                    }
                }
            }
        }

        let index = build_index(conn, model_id, dimensions, vector_count)?;
        if let Some(dir) = sidecar_dir {
            save_sidecar(
                dir,
                &index,
                model_id,
                dimensions,
                generation,
                database_id,
                0,
            )?;
        }

        Ok(Some(Self { index, dimensions }))
    }

    pub fn memory_usage_bytes(&self) -> usize {
        self.index.memory_usage()
    }

    pub fn candidates(
        &self,
        query: &[f32],
        limit: usize,
        allowed: Option<&HashSet<i64>>,
    ) -> Result<Option<Vec<i64>>> {
        if query.len() != self.dimensions || limit == 0 {
            return Ok(Some(Vec::new()));
        }
        let target = limit
            .saturating_mul(ANN_CANDIDATE_MULTIPLIER)
            .max(256)
            .min(ANN_MAX_CANDIDATES)
            .min(self.index.size());
        if target == 0 {
            return Ok(Some(Vec::new()));
        }

        let matches = match allowed {
            Some(allowed) if allowed.len() <= SMALL_FILTER_EXACT_LIMIT => {
                return Ok(Some(allowed.iter().copied().collect()));
            }
            Some(allowed) => self
                .index
                .filtered_search(query, target, |key| allowed.contains(&(key as i64)))?,
            None => self.index.search(query, target)?,
        };
        let candidates = matches
            .keys
            .into_iter()
            .map(|key| key as i64)
            .collect::<Vec<_>>();
        if candidates.len() < limit {
            if let Some(allowed) = allowed {
                if allowed.len() > SMALL_FILTER_EXACT_LIMIT {
                    // Let SQL do an exact filtered scan. Expanding a large
                    // allowlist into an IN clause can exceed SQLite's bind limit.
                    return Ok(None);
                }
                return Ok(Some(allowed.iter().copied().collect()));
            }
        }
        Ok(Some(candidates))
    }
}

pub fn active_vector_count(conn: &Connection, model_id: &str) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_embeddings WHERE model_id = ?1",
        params![model_id],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as usize)
}

#[cfg(test)]
mod tests;
