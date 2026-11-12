//! Persistent knowledge store for cross-session semantic search.
//!
//! Indexes directories of files (markdown, code, documentation) into a durable
//! SQLite database with FTS5 + vector search. Chunking and embedding are shared
//! with the rest of the crate; the store lives in one global database.

pub(crate) mod analysis;
pub mod ann;
pub mod enrichment;
pub(crate) mod entries;
pub mod filter;
pub mod findings;
pub mod freshness;
pub(crate) mod identifiers;
pub mod links;
pub(crate) mod lock;
pub mod query;
pub(crate) mod rebuild;
pub(crate) mod references;
pub(crate) mod resolution;
pub mod schema;
pub mod scope;
mod sources;
pub mod symbols;
pub mod sync;
mod vectors;

pub use sources::{KnowledgeSourceInfo, RemoveResult};
use vectors::VectorIndexState;

use anyhow::Result;
use rusqlite::Connection;
use std::sync::Arc;

use crate::embed::cache::QueryEmbeddingCache;
use crate::embed::Embed;

/// Persistent knowledge store — indexes directories of files for durable
/// cross-session search.
pub struct KnowledgeStore {
    conn: Connection,
    embedder: Option<Arc<dyn Embed>>,
    vector_index_dir: Option<std::path::PathBuf>,
    vector_index: Option<ann::VectorIndex>,
    vector_index_state: Option<VectorIndexState>,
    fuzzy_cache: query::VocabularyCache,
    query_embedding_cache: QueryEmbeddingCache,
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
            fuzzy_cache: Default::default(),
            query_embedding_cache: Default::default(),
        })
    }

    /// Set the embedder after construction.
    pub fn set_embedder(&mut self, embedder: Arc<dyn Embed>) {
        self.embedder = Some(embedder);
    }

    /// Set how many query embeddings are kept in memory. Zero disables the cache.
    /// Replacing the capacity discards existing entries.
    pub fn set_query_embedding_cache_capacity(&mut self, capacity: usize) {
        self.query_embedding_cache = QueryEmbeddingCache::new(capacity);
    }

    /// The query embedding cache. Its counters are read by benchmarks and tests.
    pub(crate) fn query_embedding_cache(&self) -> &QueryEmbeddingCache {
        &self.query_embedding_cache
    }

    /// Get a reference to the underlying SQLite connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Open a database file at `path`, creating it if needed. Used by the watcher and by tests.
    pub(crate) fn open_at(path: &std::path::Path) -> anyhow::Result<KnowledgeStore> {
        let conn = crate::db::open_db(path)?;
        schema::apply_schema(&conn)?;
        Ok(Self {
            conn,
            embedder: None,
            vector_index_dir: path.parent().map(|parent| parent.join("ann-indexes")),
            vector_index: None,
            vector_index_state: None,
            fuzzy_cache: Default::default(),
            query_embedding_cache: Default::default(),
        })
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
        fuzzy_cache: Default::default(),
        query_embedding_cache: Default::default(),
    })
}

// Test hook: fail `remove_source` after the given number of completed steps (one per chunk,
// then the symbol, metadata, and source deletes), to prove that a failure part-way leaves the
// source intact. Thread-local, like `REBUILD_FAILPOINT` in `identifiers.rs`.
#[cfg(test)]
thread_local! {
    pub(crate) static REMOVE_FAILPOINT: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn remove_failpoint(step: &mut usize) -> Result<()> {
    *step += 1;
    if REMOVE_FAILPOINT.with(|point| point.get()) == Some(*step) {
        anyhow::bail!("injected failure after remove step {step}");
    }
    Ok(())
}

#[cfg(not(test))]
fn remove_failpoint(_step: &mut usize) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;

/// Cross-module integration tests exercising the full knowledge store pipeline:
/// register → sync (with enrichment) → search (with filters) → reindex → remove.
///
/// These verify the spec's "Verification Contracts" and "Test Anchors" that span
/// multiple modules (P01 through P06).
#[cfg(test)]
mod integration;
