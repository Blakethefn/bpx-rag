//! Rebuildable USearch HNSW sidecar for large active-model vector sets.
//!
//! SQLite remains the source of truth. This index is a derived candidate
//! generator; every returned row ID is rechecked and exactly re-ranked in SQL.

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::embedder;

const SIDECAR_VERSION: u32 = 1;
const ANN_MIN_VECTORS: usize = 4_096;
const ANN_MAX_CANDIDATES: usize = 8_192;
const ANN_CANDIDATE_MULTIPLIER: usize = 512;
const SMALL_FILTER_EXACT_LIMIT: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    database_id: String,
    model_id: String,
    dimensions: usize,
    generation: i64,
    tombstones: usize,
}

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

fn build_index(
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
        let vector = embedder::bytes_to_embedding(&bytes);
        if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
            anyhow::bail!("invalid vector contents for embedding row {rowid}");
        }
        index
            .add(rowid as u64, &vector)
            .map_err(|error| anyhow!("failed to add row {rowid} to HNSW index: {error}"))?;
    }
    Ok(index)
}

fn apply_incremental_changes(
    conn: &Connection,
    index: &Index,
    model_id: &str,
    dimensions: usize,
    from_generation: i64,
    to_generation: i64,
    previous_tombstones: usize,
    expected_size: usize,
) -> Result<Option<usize>> {
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
            let vector = embedder::bytes_to_embedding(&bytes);
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

fn load_sidecar(
    dir: &Path,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
) -> Result<Option<(Index, Manifest)>> {
    let manifest_path = manifest_path(dir, model_id)?;
    let Ok(manifest_bytes) = fs::read(&manifest_path) else {
        return Ok(None);
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&manifest_bytes) else {
        return Ok(None);
    };
    if manifest.version != SIDECAR_VERSION
        || manifest.model_id != model_id
        || manifest.dimensions != dimensions
        || manifest.generation > generation
        || manifest.database_id != database_id
    {
        return Ok(None);
    }
    let index_path = index_path(dir, model_id, database_id, manifest.generation)?;
    if !index_path.is_file() {
        return Ok(None);
    }
    match Index::restore(&index_path.to_string_lossy()) {
        Ok(index)
            if index.dimensions() == dimensions
                && index.metric_kind() == MetricKind::Cos
                && index.scalar_kind() == ScalarKind::F32 =>
        {
            Ok(Some((index, manifest)))
        }
        Ok(_) => Ok(None),
        Err(error) => {
            eprintln!("[bpx-rag] ignoring invalid HNSW sidecar: {error}");
            Ok(None)
        }
    }
}

fn save_sidecar(
    dir: &Path,
    index: &Index,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
    tombstones: usize,
) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create ANN sidecar directory {}", dir.display()))?;
    let path = index_path(dir, model_id, database_id, generation)?;
    let temporary_path = sidecar_temp_path(&path);
    index
        .save(&temporary_path.to_string_lossy())
        .map_err(|error| anyhow!("failed to save HNSW sidecar: {error}"))?;
    fs::rename(&temporary_path, &path)
        .with_context(|| format!("failed to install ANN sidecar {}", path.display()))?;

    let manifest = Manifest {
        version: SIDECAR_VERSION,
        database_id: database_id.to_string(),
        model_id: model_id.to_string(),
        dimensions,
        generation,
        tombstones,
    };
    let manifest_path = manifest_path(dir, model_id)?;
    let temporary_manifest_path = sidecar_temp_path(&manifest_path);
    fs::write(&temporary_manifest_path, serde_json::to_vec(&manifest)?)?;
    fs::rename(&temporary_manifest_path, &manifest_path).with_context(|| {
        format!(
            "failed to install ANN sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    remove_old_sidecars(dir, model_id, &path)?;
    Ok(())
}

fn model_hash(model_id: &str) -> String {
    format!("{:x}", Sha256::digest(model_id.as_bytes()))
}

fn manifest_path(dir: &Path, model_id: &str) -> Result<PathBuf> {
    Ok(dir.join(format!("{}.manifest.json", model_hash(model_id))))
}

fn index_path(dir: &Path, model_id: &str, database_id: &str, generation: i64) -> Result<PathBuf> {
    Ok(dir.join(format!(
        "{}-{}-{}.usearch",
        model_hash(model_id),
        &database_id[..database_id.len().min(12)],
        generation
    )))
}

fn sidecar_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("index");
    path.with_file_name(format!("{name}.tmp-{}", std::process::id()))
}

fn remove_old_sidecars(dir: &Path, model_id: &str, current: &Path) -> Result<()> {
    let prefix = model_hash(model_id);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == current {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".usearch") {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!(
                    "[bpx-rag] could not remove stale ANN sidecar {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DIMENSIONS: usize = 32;
    const TEST_MODEL: &str = "test/model-space-v1";

    fn vector_for(index: usize) -> Vec<f32> {
        let mut state = (index as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut vector = (0..TEST_DIMENSIONS)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as f32 / ((1_u32 << 24) as f32)) - 0.5
            })
            .collect::<Vec<_>>();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        vector.iter_mut().for_each(|value| *value /= norm);
        vector
    }

    fn sample_database(count: usize) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE knowledge_embeddings (
                chunk_rowid INTEGER PRIMARY KEY,
                embedding BLOB NOT NULL,
                dim INTEGER NOT NULL,
                model_id TEXT NOT NULL
             );",
        )
        .unwrap();
        for index in 0..count {
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    index as i64 + 1,
                    embedder::embedding_to_bytes(&vector_for(index)),
                    TEST_DIMENSIONS as i64,
                    TEST_MODEL
                ],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn hnsw_candidate_search_recalls_exact_neighbors_and_roundtrips_sidecar() {
        let count = 4096;
        let conn = sample_database(count);
        let index = VectorIndex {
            index: build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap(),
            dimensions: TEST_DIMENSIONS,
        };
        let query = vector_for(17);
        let candidates = index.candidates(&query, 10, None).unwrap().unwrap();
        let exact = (0..count)
            .map(|idx| (idx, vector_for(idx)))
            .map(|(idx, vector)| {
                let score = query
                    .iter()
                    .zip(vector.iter())
                    .map(|(a, b)| a * b)
                    .sum::<f32>();
                (idx as i64 + 1, score)
            })
            .collect::<Vec<_>>();
        let mut exact = exact;
        exact.sort_by(|left, right| right.1.total_cmp(&left.1));
        let exact_top = exact
            .iter()
            .take(10)
            .map(|(rowid, _)| *rowid)
            .collect::<HashSet<_>>();
        let candidate_set = candidates.iter().copied().collect::<HashSet<_>>();
        let overlap = exact_top.intersection(&candidate_set).count();
        assert!(overlap >= 9, "ANN candidate recall was {overlap}/10");

        let dir = tempfile::tempdir().unwrap();
        save_sidecar(
            dir.path(),
            &index.index,
            TEST_MODEL,
            TEST_DIMENSIONS,
            7,
            "test-database-instance",
            0,
        )
        .unwrap();
        let (restored, manifest) = load_sidecar(
            dir.path(),
            TEST_MODEL,
            TEST_DIMENSIONS,
            7,
            "test-database-instance",
        )
        .unwrap()
        .unwrap();
        assert_eq!(restored.size(), count);
        assert_eq!(restored.dimensions(), TEST_DIMENSIONS);
        assert_eq!(manifest.tombstones, 0);
    }

    #[test]
    fn filtered_hnsw_underfill_falls_back_without_expanding_large_allowlist() {
        let count = 4096;
        let conn = sample_database(count);
        let index = VectorIndex {
            index: build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap(),
            dimensions: TEST_DIMENSIONS,
        };
        let allowed = (10_000..10_513).collect::<HashSet<_>>();

        let candidates = index
            .candidates(&vector_for(0), 2000, Some(&allowed))
            .unwrap();

        assert!(candidates.is_none());
    }

    #[test]
    fn sidecar_manifest_rejects_stale_generation_and_database_identity() {
        let count = 8;
        let conn = sample_database(count);
        let index = build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap();
        let dir = tempfile::tempdir().unwrap();
        save_sidecar(
            dir.path(),
            &index,
            TEST_MODEL,
            TEST_DIMENSIONS,
            3,
            "database-a",
            0,
        )
        .unwrap();

        let (_, manifest) = load_sidecar(dir.path(), TEST_MODEL, TEST_DIMENSIONS, 4, "database-a")
            .unwrap()
            .unwrap();
        assert_eq!(manifest.generation, 3);
        assert!(
            load_sidecar(dir.path(), TEST_MODEL, TEST_DIMENSIONS, 3, "database-b")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn saved_sidecar_applies_incremental_embedding_updates_and_deletes() {
        let count = 5000;
        let conn = sample_database(count);
        conn.execute_batch(
            "CREATE TABLE knowledge_embedding_state (
                state_id INTEGER PRIMARY KEY,
                generation INTEGER NOT NULL,
                database_id TEXT NOT NULL
             );
             INSERT INTO knowledge_embedding_state VALUES (1, 0, 'database-incremental');
             CREATE TABLE knowledge_embedding_changes (
                change_id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_rowid INTEGER NOT NULL,
                model_id TEXT NOT NULL,
                operation TEXT NOT NULL
             );
             CREATE TRIGGER changed_embedding_update AFTER UPDATE ON knowledge_embeddings BEGIN
                INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                    VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert');
                UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
             END;
             CREATE TRIGGER changed_embedding_delete AFTER DELETE ON knowledge_embeddings BEGIN
                INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                    VALUES (OLD.chunk_rowid, OLD.model_id, 'delete');
                UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
             END;",
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let index = VectorIndex::load_or_build(
            &conn,
            Some(dir.path()),
            TEST_MODEL,
            TEST_DIMENSIONS,
            0,
            "database-incremental",
        )
        .unwrap()
        .unwrap();
        assert_eq!(index.index.size(), count);

        let replacement = vector_for(count + 10);
        conn.execute(
            "UPDATE knowledge_embeddings SET embedding = ?1 WHERE chunk_rowid = 1",
            params![embedder::embedding_to_bytes(&replacement)],
        )
        .unwrap();
        let generation: i64 = conn
            .query_row(
                "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let index = VectorIndex::load_or_build(
            &conn,
            Some(dir.path()),
            TEST_MODEL,
            TEST_DIMENSIONS,
            generation,
            "database-incremental",
        )
        .unwrap()
        .unwrap();
        assert!(index
            .candidates(&replacement, 10, None)
            .unwrap()
            .unwrap()
            .contains(&1));

        conn.execute("DELETE FROM knowledge_embeddings WHERE chunk_rowid = 2", [])
            .unwrap();
        let generation: i64 = conn
            .query_row(
                "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let index = VectorIndex::load_or_build(
            &conn,
            Some(dir.path()),
            TEST_MODEL,
            TEST_DIMENSIONS,
            generation,
            "database-incremental",
        )
        .unwrap()
        .unwrap();
        assert_eq!(index.index.size(), count - 1);
        assert!(!index.index.contains(2));
    }
}
