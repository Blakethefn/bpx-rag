use anyhow::{anyhow, Result};

use crate::config::Config;
use crate::knowledge::lock::FileLock;
use crate::knowledge::{self, KnowledgeStore};
use crate::search::profile::SearchProfile;
use crate::search::{self, QueryRequest};
use crate::watch::{self, RunOutcome, WatchOptions};

mod backfill;
mod embed;
mod read;
mod sources;
mod status;
mod symbols;

use embed::add_embedding_notice;
use sources::validate_label;

pub use embed::{embedding_model_status, load_watch_embedder, model_directory};
pub use sources::format_sync_result;

/// Largest number of symbol matches one lookup returns.
pub const MAX_SYMBOL_LIMIT: usize = 100;

pub struct RagApp {
    config: Config,
    knowledge: KnowledgeStore,
    embedding_notice: Option<String>,
    embedder_loaded: bool,
}

#[derive(Debug, Clone)]
pub struct SearchOptions<'a> {
    pub source: Option<&'a str>,
    pub filter: Option<&'a str>,
    pub limit: u32,
    pub max_per_file: u32,
    pub profile: Option<&'a str>,
    pub explain: bool,
    pub count_only: bool,
}

impl Default for SearchOptions<'_> {
    fn default() -> Self {
        Self {
            source: None,
            filter: None,
            limit: 10,
            max_per_file: 1,
            profile: None,
            explain: false,
            count_only: false,
        }
    }
}

impl RagApp {
    pub fn open() -> Result<Self> {
        let config = Config::load()?;
        let knowledge = KnowledgeStore::open()?;
        let embedding_notice = embed::embedding_notice(&config);
        Ok(Self {
            config,
            knowledge,
            embedding_notice,
            embedder_loaded: false,
        })
    }

    pub fn search(&mut self, query: &str, options: SearchOptions<'_>) -> Result<String> {
        let query = validate_query(query)?;
        let profile = SearchProfile::parse(options.profile)?;
        self.ensure_embedder();
        if let Some((model_id, dimensions)) = self
            .knowledge
            .embedder()
            .map(|embedder| (embedder.model_id().to_string(), embedder.dim()))
        {
            if let Err(error) = self.knowledge.ensure_vector_index(&model_id, dimensions) {
                self.knowledge.clear_vector_index();
                self.embedding_notice = Some(format!(
                    "Approximate index unavailable; using exact vector search: {error}"
                ));
                eprintln!("[bpx-rag] failed to prepare approximate index: {error}");
            }
        }
        if let Some(notice) = self.embedding_coverage_notice()? {
            self.embedding_notice = Some(notice);
        }
        let outcome = search::run_query(
            QueryRequest {
                query,
                source: options.source,
                knowledge_filter: options.filter,
                limit: options.limit.clamp(1, 50),
                max_per_file: options.max_per_file,
                snippet_bytes: self.config.search.snippet_bytes,
                profile,
                explain: options.explain,
                vector_unavailable: if self.knowledge.embedder().is_none() {
                    self.embedding_notice.as_deref()
                } else {
                    None
                },
            },
            &self.config,
            &self.knowledge,
            options.count_only,
        )?;
        Ok(add_embedding_notice(
            outcome,
            self.embedding_notice.as_deref(),
            options.count_only,
        ))
    }

    pub fn findings(&self, source: Option<&str>) -> Result<String> {
        knowledge::findings::report(&self.knowledge, source)
    }

    /// Turn watch mode on or off for a source. A running watcher picks the change up within a
    /// second or two.
    pub fn set_watch(&self, label: &str, enabled: bool) -> Result<String> {
        validate_label(label)?;
        let _writer = self.writer_lock()?;
        self.knowledge.set_watch(label, enabled)?;
        Ok(format!(
            "Watch mode {} for source '{label}'.",
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    /// Run the watcher in this process until it is stopped. Returns `AlreadyRunning` when another
    /// process holds the watcher lease.
    pub fn run_watch(&self) -> Result<RunOutcome> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        let stop = std::sync::atomic::AtomicBool::new(false);
        watch::run_with_loader(
            &db_path,
            &WatchOptions::default(),
            &stop,
            Box::new(load_watch_embedder),
        )
    }

    pub(in crate::app) fn writer_lock(&self) -> Result<FileLock> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        FileLock::acquire(&knowledge::lock::writer_lock_path(&db_path))
    }
}

fn validate_query(query: &str) -> Result<&str> {
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("search query cannot be empty"));
    }
    if query.len() > 16_384 {
        return Err(anyhow!("search query exceeds the 16 KiB limit"));
    }
    Ok(query)
}
