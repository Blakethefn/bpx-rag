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

pub struct KnowledgeStore {
    conn: Connection,
    embedder: Option<Arc<dyn Embed>>,
    vector_index_dir: Option<std::path::PathBuf>,
    vector_index: Option<ann::VectorIndex>,
    vector_index_state: Option<VectorIndexState>,
    fuzzy_cache: query::VocabularyCache,
    query_embedding_cache: QueryEmbeddingCache,
}

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

    pub fn set_embedder(&mut self, embedder: Arc<dyn Embed>) {
        self.embedder = Some(embedder);
    }

    pub fn set_query_embedding_cache_capacity(&mut self, capacity: usize) {
        self.query_embedding_cache = QueryEmbeddingCache::new(capacity);
    }

    pub(crate) fn query_embedding_cache(&self) -> &QueryEmbeddingCache {
        &self.query_embedding_cache
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

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

    pub fn embedder(&self) -> Option<&Arc<dyn Embed>> {
        self.embedder.as_ref()
    }
}

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

#[cfg(test)]
mod integration;
