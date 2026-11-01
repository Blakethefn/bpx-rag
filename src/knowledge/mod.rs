//! Persistent knowledge store for cross-session semantic search.
//!
//! Indexes directories of files (markdown, code, documentation) into a durable
//! SQLite database with FTS5 + vector search. Chunking and embedding are shared
//! with the rest of the crate; the store lives in one global database.

pub mod ann;
pub mod enrichment;
pub mod filter;
pub mod links;
pub mod query;
pub mod schema;
pub mod scope;
pub mod sync;

use anyhow::{anyhow, Result};
use rusqlite::{Connection, OptionalExtension};
use std::sync::Arc;

use crate::embed::Embed;

/// Persistent knowledge store — indexes directories of files for durable
/// cross-session search.
pub struct KnowledgeStore {
    conn: Connection,
    embedder: Option<Arc<dyn Embed>>,
    vector_index_dir: Option<std::path::PathBuf>,
    vector_index: Option<ann::VectorIndex>,
    vector_index_state: Option<VectorIndexState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VectorIndexState {
    model_id: String,
    dimensions: usize,
    generation: i64,
    database_id: String,
}

/// A registered knowledge source.
#[derive(Debug, Clone)]
pub struct KnowledgeSourceInfo {
    pub id: i64,
    pub label: String,
    pub path: String,
    pub patterns: Vec<String>,
    pub enrichments: Vec<String>,
    pub created_at: String,
    pub last_sync: Option<String>,
    pub file_count: i64,
    pub chunk_count: i64,
}

/// Result of removing a knowledge source.
#[derive(Debug)]
pub struct RemoveResult {
    pub files_removed: i64,
    pub chunks_removed: i64,
}

/// A chunk discovered through graph traversal.
#[derive(Debug, Clone)]
pub struct RelatedChunk {
    pub file_id: i64,
    pub rel_path: String,
    pub title: String,
    pub content: String,
    pub line_start: u32,
    pub line_end: u32,
}

/// Parse a JSON-encoded enrichments array. Sources write this column as JSON,
/// so a malformed value is a corrupt row and is returned as a decode error.
fn parse_enrichments(json: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })
}

/// Map a SQLite row to KnowledgeSourceInfo. Columns must be in the order:
/// id, label, path, glob, enrichments, created_at, last_sync, file_count, chunk_count
fn row_to_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeSourceInfo> {
    let enrichments_json: String = row.get(4)?;
    Ok(KnowledgeSourceInfo {
        id: row.get(0)?,
        label: row.get(1)?,
        path: row.get(2)?,
        patterns: scope::decode(row.get::<_, Option<String>>(3)?.as_deref()),
        enrichments: parse_enrichments(&enrichments_json)?,
        created_at: row.get(5)?,
        last_sync: row.get(6)?,
        file_count: row.get(7)?,
        chunk_count: row.get(8)?,
    })
}

const SOURCE_COLUMNS: &str =
    "id, label, path, glob, enrichments, created_at, last_sync, file_count, chunk_count";

impl KnowledgeStore {
    /// Open the knowledge store, creating the DB if needed.
    pub fn open() -> Result<Self> {
        let db_path = schema::knowledge_db_path()?;
        let conn = crate::db::open_db(&db_path)?;
        schema::apply_schema(&conn)?;
        let vector_index_dir = db_path.parent().map(|parent| parent.join("ann-indexes"));
        Ok(Self {
            conn,
            embedder: None,
            vector_index_dir,
            vector_index: None,
            vector_index_state: None,
        })
    }

    /// Set the embedder after construction.
    pub fn set_embedder(&mut self, embedder: Arc<dyn Embed>) {
        self.embedder = Some(embedder);
    }

    /// Register a new knowledge source. Returns the source ID.
    /// Errors if the label already exists.
    pub fn add_source(
        &self,
        label: &str,
        path: &str,
        patterns: &[String],
        enrichments: &[String],
    ) -> Result<i64> {
        let enrichments_json = serde_json::to_string(enrichments)?;
        let stored_scope = scope::encode(patterns);
        self.conn.execute(
            "INSERT INTO knowledge_sources (label, path, glob, enrichments)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![label, path, stored_scope, enrichments_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove a knowledge source and all its indexed content.
    /// FTS5 virtual tables don't support ON DELETE CASCADE so chunks are
    /// removed manually before deleting the source record.
    pub fn remove_source(&self, label: &str) -> Result<RemoveResult> {
        let source = self
            .get_source(label)?
            .ok_or_else(|| anyhow!("Knowledge source '{}' not found", label))?;

        let source_id = source.id;

        // Collect chunk rowids before any deletions so we still have references.
        let chunk_rowids: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE source_id = ?1")?;
            let rows = stmt.query_map(rusqlite::params![source_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let chunks_removed = chunk_rowids.len() as i64;

        let files_removed: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1",
            rusqlite::params![source_id],
            |row| row.get(0),
        )?;

        // Delete from FTS5 virtual tables first (no FK cascade support).
        for rowid in &chunk_rowids {
            self.conn.execute(
                "DELETE FROM knowledge_chunks WHERE rowid = ?1",
                rusqlite::params![rowid],
            )?;
            self.conn.execute(
                "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
                rusqlite::params![rowid],
            )?;
            self.conn.execute(
                "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
                rusqlite::params![rowid],
            )?;
        }

        // Delete chunk_meta before the source (FK references files and source).
        self.conn.execute(
            "DELETE FROM knowledge_chunk_meta WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;

        // Delete the source — ON DELETE CASCADE removes knowledge_files.
        self.conn.execute(
            "DELETE FROM knowledge_sources WHERE id = ?1",
            rusqlite::params![source_id],
        )?;

        Ok(RemoveResult {
            files_removed,
            chunks_removed,
        })
    }

    /// List all registered knowledge sources.
    pub fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SOURCE_COLUMNS} FROM knowledge_sources ORDER BY label"
        ))?;
        let sources: Vec<KnowledgeSourceInfo> = stmt
            .query_map([], row_to_source)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sources)
    }

    /// Find a source by label. Returns None if not found.
    pub fn get_source(&self, label: &str) -> Result<Option<KnowledgeSourceInfo>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SOURCE_COLUMNS} FROM knowledge_sources WHERE label = ?1"
        ))?;
        let result = stmt
            .query_row(rusqlite::params![label], row_to_source)
            .optional()?;
        Ok(result)
    }

    /// Get a reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn ensure_vector_index(&mut self, model_id: &str, dimensions: usize) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> Result<(VectorIndexState, Option<ann::VectorIndex>)> {
            let state = self.conn.query_row(
                "SELECT generation, database_id FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| {
                    Ok(VectorIndexState {
                        model_id: model_id.to_string(),
                        dimensions,
                        generation: row.get(0)?,
                        database_id: row.get(1)?,
                    })
                },
            )?;
            if self.vector_index_state.as_ref() == Some(&state) {
                return Ok((state, None));
            }
            let index = ann::VectorIndex::load_or_build(
                &self.conn,
                self.vector_index_dir.as_deref(),
                &state.model_id,
                state.dimensions,
                state.generation,
                &state.database_id,
            )?;
            Ok((state, index))
        })();
        match result {
            Ok((state, index)) => {
                self.conn.execute_batch("COMMIT")?;
                if self.vector_index_state.as_ref() != Some(&state) {
                    self.vector_index = index;
                    self.vector_index_state = Some(state);
                }
                Ok(())
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn clear_vector_index(&mut self) {
        self.vector_index = None;
        self.vector_index_state = None;
    }

    pub(crate) fn vector_index_memory_usage(&self) -> usize {
        self.vector_index
            .as_ref()
            .map_or(0, ann::VectorIndex::memory_usage_bytes)
    }

    pub(crate) fn vector_candidates(
        &self,
        model_id: &str,
        query_embedding: &[f32],
        limit: u32,
        predicates: &[filter::FilterPredicate],
    ) -> Result<Option<Vec<i64>>> {
        let Some(index) = &self.vector_index else {
            return Ok(None);
        };
        let Some(state) = &self.vector_index_state else {
            return Ok(None);
        };
        if state.model_id != model_id || state.dimensions != query_embedding.len() {
            return Ok(None);
        }

        let allowed = if predicates.is_empty() {
            None
        } else {
            let (meta_where, meta_params) = filter::predicates_to_sql(predicates, 1);
            let metadata_clause = if meta_where.is_empty() {
                String::new()
            } else {
                format!(" AND {meta_where}")
            };
            let sql = format!(
                "SELECT ke.chunk_rowid
                 FROM knowledge_embeddings ke
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                 JOIN knowledge_files kf ON kf.id = kcm.file_id
                 WHERE ke.model_id = ?1{metadata_clause}"
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(model_id.to_string())];
            for value in meta_params {
                params.push(Box::new(value));
            }
            let param_refs = params
                .iter()
                .map(|param| param.as_ref())
                .collect::<Vec<_>>();
            let mut statement = self.conn.prepare(&sql)?;
            let rowids = statement
                .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
            if rowids.is_empty() {
                return Ok(Some(Vec::new()));
            }
            Some(rowids)
        };

        index.candidates(query_embedding, limit as usize, allowed.as_ref())
    }

    /// Get a reference to the embedder, if available.
    pub fn embedder(&self) -> Option<&Arc<dyn Embed>> {
        self.embedder.as_ref()
    }
}

/// Open an in-memory knowledge store for tests and isolated evaluations. Uses
/// the same schema as the on-disk store but doesn't persist to disk.
pub(crate) fn open_in_memory() -> anyhow::Result<KnowledgeStore> {
    use crate::db::open_db;
    use std::path::PathBuf;
    let conn = open_db(&PathBuf::from(":memory:"))?;
    crate::knowledge::schema::apply_schema(&conn)?;
    Ok(KnowledgeStore {
        conn,
        embedder: None,
        vector_index_dir: None,
        vector_index: None,
        vector_index_state: None,
    })
}

#[cfg(test)]
mod tests;

/// Cross-module integration tests exercising the full knowledge store pipeline:
/// register → sync (with enrichment) → search (with filters) → reindex → remove.
///
/// These verify the spec's "Verification Contracts" and "Test Anchors" that span
/// multiple modules (P01 through P06).
#[cfg(test)]
mod integration_tests;
