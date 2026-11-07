use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge::lock::FileLock;
use crate::knowledge::symbols::{self, MatchMode, SymbolQuery, SymbolReport};
use crate::knowledge::{self, KnowledgeSourceInfo, KnowledgeStore};
use crate::search::profile::SearchProfile;
use crate::search::{self, QueryRequest};
use crate::watch::{self, Holder, RunOutcome, WatchOptions};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingBackfillReport {
    pub source_label: Option<String>,
    pub embedded_chunks: usize,
    pub skipped_chunks: usize,
    pub remaining_chunks: usize,
}

impl EmbeddingBackfillReport {
    pub fn summary(&self) -> String {
        let scope = self
            .source_label
            .as_deref()
            .map(|label| format!(" for source '{label}'"))
            .unwrap_or_else(|| " across all sources".to_string());
        format!(
            "Embedding backfill{scope}: wrote {} active-model vectors, skipped {} stale or already-completed chunks, {} remain.",
            self.embedded_chunks, self.skipped_chunks, self.remaining_chunks
        )
    }
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
        let embedding_notice = embedding_notice(&config);
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

    pub fn add_source(
        &mut self,
        path: &Path,
        label: &str,
        include: &[String],
        exclude: &[String],
        enrichments: &[String],
    ) -> Result<String> {
        validate_label(label)?;
        knowledge::enrichment::validate_enrichments(enrichments)?;
        let _writer = self.writer_lock()?;
        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("could not resolve source path {}", path.display()))?;
        if !canonical_path.is_dir() {
            return Err(anyhow!("source path must be a directory"));
        }
        let patterns = knowledge::scope::patterns_from(include, exclude);
        knowledge::scope::validate(&canonical_path, &patterns)?;
        if self.knowledge.get_source(label)?.is_some() {
            return Err(anyhow!("a knowledge source named '{label}' already exists"));
        }
        self.ensure_embedder();
        self.knowledge.add_source(
            label,
            &canonical_path.to_string_lossy(),
            &patterns,
            enrichments,
        )?;
        // Registration and the first sync succeed or fail together. A source left registered
        // with no content would also refuse a retry under the same label.
        match self.register_and_sync(label) {
            Ok(result) => Ok(format_sync_result(&result)),
            Err(error) => {
                if let Err(cleanup) = self.knowledge.remove_source(label) {
                    eprintln!(
                        "[bpx-rag] could not remove source '{label}' after its first sync failed: {cleanup:#}"
                    );
                }
                Err(error)
            }
        }
    }

    /// The first sync of a source that was just registered.
    fn register_and_sync(&self, label: &str) -> Result<knowledge::sync::SyncResult> {
        let source = self
            .knowledge
            .get_source(label)?
            .ok_or_else(|| anyhow!("new source could not be loaded"))?;
        self.sync_one(&source)
    }

    pub fn sync_sources(
        &mut self,
        label: Option<&str>,
    ) -> Result<Vec<knowledge::sync::SyncResult>> {
        let sources = match label {
            Some(label) => vec![self
                .knowledge
                .get_source(label)?
                .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?],
            None => self.knowledge.list_sources()?,
        };
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let _writer = self.writer_lock()?;
        self.ensure_embedder();
        sources.iter().map(|source| self.sync_one(source)).collect()
    }

    pub fn find_symbols(
        &self,
        name: &str,
        mode: MatchMode,
        source: Option<&str>,
        limit: usize,
    ) -> Result<String> {
        if !(1..=MAX_SYMBOL_LIMIT).contains(&limit) {
            return Err(anyhow!("limit must be between 1 and {MAX_SYMBOL_LIMIT}"));
        }
        let name = name.trim();
        let report = symbols::lookup(
            &self.knowledge,
            &SymbolQuery {
                name,
                mode,
                source,
                limit,
            },
        )?;
        Ok(format_symbol_report(name, mode, &report))
    }

    pub fn remove_source(&self, label: &str) -> Result<knowledge::RemoveResult> {
        validate_label(label)?;
        let _writer = self.writer_lock()?;
        self.knowledge.remove_source(label)
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

    /// The watcher lease holder and each source's watch setting and last reported state.
    pub fn watch_status(&self) -> Result<String> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        let holder = watch::holder(&db_path)?;
        let status = watch::read_status(&db_path)?;
        let watched: Vec<String> = self
            .knowledge
            .watched_sources()?
            .into_iter()
            .map(|source| source.label)
            .collect();
        let mut lines = vec![match holder {
            Holder::Running { pid } => format!(
                "watcher: running (pid {})",
                pid.map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
            ),
            Holder::NotRunning => "watcher: not running".to_string(),
        }];
        let running = matches!(holder, Holder::Running { .. });
        for source in self.knowledge.list_sources()? {
            if !watched.contains(&source.label) {
                lines.push(format!("{}: watch disabled", source.label));
                continue;
            }
            let reported = status.as_ref().and_then(|status| {
                status
                    .sources
                    .iter()
                    .find(|entry| entry.label == source.label)
            });
            let detail = match reported {
                Some(entry) => format!(
                    "state {}, batches {}, reindexed {}, reconciled at {}, last error {}{}{}",
                    serde_json::to_value(entry.state)?
                        .as_str()
                        .unwrap_or("unknown"),
                    entry.batches,
                    entry.reindexed_files,
                    entry
                        .reconciled_at
                        .map_or_else(|| "never".to_string(), |at| at.to_string()),
                    entry.last_error.as_deref().unwrap_or("none"),
                    if running {
                        ""
                    } else {
                        " (no watcher is running)"
                    },
                    skipped_note(&entry.skipped_files),
                ),
                None => "no watcher has reported yet".to_string(),
            };
            lines.push(format!("{}: watch enabled; {detail}", source.label));
        }
        Ok(lines.join("\n"))
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

    fn writer_lock(&self) -> Result<FileLock> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        FileLock::acquire(&knowledge::lock::writer_lock_path(&db_path))
    }

    pub fn read_chunks(
        &self,
        label: &str,
        max_chunks: usize,
        file_path: Option<&str>,
        line_start: Option<u32>,
        line_end: Option<u32>,
    ) -> Result<String> {
        validate_label(label)?;
        if line_start == Some(0) || line_end == Some(0) {
            return Err(anyhow!("line numbers must be greater than zero"));
        }
        if let (Some(start), Some(end)) = (line_start, line_end) {
            if start > end {
                return Err(anyhow!("line_start must not be greater than line_end"));
            }
        }
        if let Some(path) = file_path {
            let requested_path = Path::new(path);
            if requested_path.as_os_str().is_empty()
                || requested_path.is_absolute()
                || requested_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(anyhow!(
                    "path must be a normalized source-relative file path"
                ));
            }
        }
        let max_chunks = max_chunks.clamp(1, 100);
        let source = self
            .knowledge
            .get_source(label)?
            .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?;
        let mut statement = self.knowledge.conn().prepare(
            "SELECT kf.rel_path, kc.title, kc.content,
                    COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0)
             FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
             JOIN knowledge_files kf ON kf.id = kcm.file_id
             WHERE kcm.source_id = ?1
               AND (?2 IS NULL OR kf.rel_path = ?2)
               AND (?3 IS NULL OR kcm.line_end >= ?3)
               AND (?4 IS NULL OR kcm.line_start <= ?4)
             ORDER BY kf.rel_path, kcm.line_start, kc.rowid
             LIMIT ?5",
        )?;
        let chunks = statement.query_map(
            params![
                source.id,
                file_path,
                line_start.map(i64::from),
                line_end.map(i64::from),
                max_chunks as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        let mut output = String::new();
        for chunk in chunks {
            let (path, title, content, line_start, line_end) = chunk?;
            output.push_str(&format!(
                "## {path}:{line_start}-{line_end} — {title}\n\n{content}\n\n"
            ));
        }
        if output.is_empty() {
            return Ok(format!(
                "No indexed chunks matched the requested location in source '{label}'."
            ));
        }
        Ok(output)
    }

    pub fn source_status(&self) -> Result<String> {
        let sources = self.knowledge.list_sources()?;
        if sources.is_empty() {
            return Ok("No sources are registered. Add one with `bpx-rag add`.".into());
        }
        let mut output = String::from("# Knowledge sources\n\n");
        for source in sources {
            let model_id = embed::canonical_model_id(&self.config.embeddings.model)?;
            let (embedded, stale, total): (i64, i64, i64) = self.knowledge.conn().query_row(
                "SELECT COUNT(DISTINCT CASE WHEN e.model_id = ?1 THEN m.chunk_rowid END),
                        COUNT(DISTINCT CASE WHEN e.model_id IS NOT NULL AND e.model_id != ?1 THEN m.chunk_rowid END),
                        COUNT(DISTINCT m.chunk_rowid)
                 FROM knowledge_chunk_meta m
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = m.chunk_rowid
                 WHERE m.source_id = ?2",
                params![model_id, source.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            output.push_str(&format!(
                "- **{}** — {} files, {} chunks, {}/{} embedded; added {}; last sync: {}\n  Path: `{}`\n",
                source.label,
                source.file_count,
                source.chunk_count,
                embedded,
                total,
                source.created_at,
                source.last_sync.as_deref().unwrap_or("never"),
                source.path
            ));
            if stale > 0 {
                output.push_str(&format!(
                    "  Warning: {stale} chunks have vectors from another model and are not searchable semantically; run `bpx-rag embed --label {}`\n",
                    source.label
                ));
            }
        }
        output.push_str(&format!(
            "\nEmbedding model: `{}` ({})\n",
            self.config.embeddings.model,
            self.embedding_model_status()
        ));
        Ok(output)
    }

    pub fn backfill_embeddings(
        &mut self,
        source_label: Option<&str>,
    ) -> Result<EmbeddingBackfillReport> {
        let _writer = self.writer_lock()?;
        let source = match source_label {
            Some(label) if label.trim().is_empty() => {
                return Err(anyhow!("source label must not be empty"));
            }
            Some(label) => Some(
                self.knowledge
                    .get_source(label)?
                    .ok_or_else(|| anyhow!("knowledge source '{label}' not found"))?,
            ),
            None => None,
        };
        let source_id = source.as_ref().map(|source| source.id);
        self.ensure_embedder();
        let embedder = self.knowledge.embedder().ok_or_else(|| {
            anyhow!("embedding model is unavailable; run `bpx-rag models download` first")
        })?;
        let conn = self.knowledge.conn();
        let batch_size = self.config.embeddings.batch_size.clamp(1, 256);
        let mut report = EmbeddingBackfillReport {
            source_label: source.map(|source| source.label),
            embedded_chunks: 0,
            skipped_chunks: 0,
            remaining_chunks: 0,
        };
        loop {
            let batch = {
                let row_mapper = |row: &rusqlite::Row<'_>| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                };
                match source_id {
                    Some(source_id) => {
                        let mut statement = conn.prepare(
                            "SELECT kc.rowid, kc.title, kc.content, m.source_id
                             FROM knowledge_chunks kc
                             JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                             WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
                               AND m.source_id = ?2
                             ORDER BY kc.rowid
                             LIMIT ?3",
                        )?;
                        let rows = statement
                            .query_map(
                                params![embedder.model_id(), source_id, batch_size as i64],
                                row_mapper,
                            )?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        rows
                    }
                    None => {
                        let mut statement = conn.prepare(
                            "SELECT kc.rowid, kc.title, kc.content, m.source_id
                             FROM knowledge_chunks kc
                             JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                             WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1
                             ORDER BY kc.rowid
                             LIMIT ?2",
                        )?;
                        let rows = statement
                            .query_map(params![embedder.model_id(), batch_size as i64], row_mapper)?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        rows
                    }
                }
            };
            if batch.is_empty() {
                break;
            }
            let texts = batch
                .iter()
                .map(|(_, title, content, _)| format!("{title} {content}"))
                .collect::<Vec<_>>();
            let text_refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
            let vectors = embedder.embed_batch(&text_refs)?;
            if vectors.len() != batch.len() {
                return Err(anyhow!(
                    "embedding provider returned an unexpected batch size"
                ));
            }
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let transaction_result = (|| -> Result<(usize, usize)> {
                let mut written = 0usize;
                let mut skipped = 0usize;
                for ((rowid, expected_title, expected_content, expected_source_id), vector) in
                    batch.iter().zip(vectors.iter())
                {
                    let current: Option<(String, String, i64, Option<String>)> = conn
                        .query_row(
                            "SELECT kc.title, kc.content, m.source_id, e.model_id
                             FROM knowledge_chunks kc
                             JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                             WHERE kc.rowid = ?1",
                            params![rowid],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                        )
                        .optional()?;
                    let Some((title, content, current_source_id, current_model_id)) = current
                    else {
                        skipped += 1;
                        continue;
                    };
                    if title != *expected_title
                        || content != *expected_content
                        || current_source_id != *expected_source_id
                        || current_model_id.as_deref() == Some(embedder.model_id())
                    {
                        skipped += 1;
                        continue;
                    }

                    let vector = embed::normalize_embedding(vector, embedder.dim())?;
                    conn.execute(
                        "INSERT OR REPLACE INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            *rowid,
                            embed::embedding_to_bytes(&vector),
                            embedder.dim() as i64,
                            embedder.model_id()
                        ],
                    )?;
                    written += 1;
                }
                Ok((written, skipped))
            })();
            match transaction_result {
                Ok((written, skipped)) => {
                    conn.execute_batch("COMMIT")?;
                    report.embedded_chunks += written;
                    report.skipped_chunks += skipped;
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        }
        let remaining = match source_id {
            Some(source_id) => conn.query_row(
                "SELECT COUNT(*)
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
                   AND m.source_id = ?2",
                params![embedder.model_id(), source_id],
                |row| row.get::<_, i64>(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*)
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1",
                params![embedder.model_id()],
                |row| row.get::<_, i64>(0),
            )?,
        };
        report.remaining_chunks = remaining as usize;
        Ok(report)
    }

    pub fn scrub_sensitive_index(
        &self,
        source_label: Option<&str>,
    ) -> Result<knowledge::sync::SensitiveIndexScrubReport> {
        let _writer = self.writer_lock()?;
        let source_id = match source_label {
            Some(label) if label.trim().is_empty() => {
                return Err(anyhow!("source label must not be empty"));
            }
            Some(label) => Some(
                self.knowledge
                    .get_source(label)?
                    .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?
                    .id,
            ),
            None => None,
        };
        knowledge::sync::scrub_sensitive_indexed_files(&self.knowledge, source_id)
    }

    pub fn embedding_model_status(&self) -> String {
        embedding_model_status(&self.config)
    }

    pub fn download_embedding_model(&mut self) -> Result<String> {
        let model_dir = model_directory(&self.config);
        let embedder = Embedder::new_with_device(
            &model_dir,
            &self.config.embeddings.model,
            self.config.embeddings.batch_size,
            self.config.embeddings.intra_threads,
            self.config.embeddings.device,
        )?;
        self.install_embedder(embedder);
        Ok(format!(
            "Model {} is ready at {}.",
            self.config.embeddings.model,
            model_dir.display()
        ))
    }

    fn sync_one(&self, source: &KnowledgeSourceInfo) -> Result<knowledge::sync::SyncResult> {
        let base = Path::new(&source.path);
        let enrichment_fn = knowledge::enrichment::build_enrichment_fn(&source.enrichments, base);
        let enrichment_ref = enrichment_fn
            .as_ref()
            .map(|function| function.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        knowledge::sync::sync_source(&self.knowledge, source, enrichment_ref)
    }

    fn ensure_embedder(&mut self) {
        if self.embedder_loaded {
            return;
        }
        if !self.config.embeddings.enabled {
            self.embedding_notice = Some("Embeddings are disabled in config.".into());
            return;
        }
        let model_dir = model_directory(&self.config);
        if !embed::model_files_present(&model_dir, &self.config.embeddings.model) {
            self.embedding_notice = Some(format!(
                "Semantic retrieval is unavailable; install the local model with `bpx-rag models download` ({}).",
                model_dir.display()
            ));
            return;
        }
        match Embedder::load_cached_with_device(
            &model_dir,
            &self.config.embeddings.model,
            self.config.embeddings.batch_size,
            self.config.embeddings.intra_threads,
            self.config.embeddings.device,
        ) {
            Ok(embedder) => self.install_embedder(embedder),
            Err(error) => {
                self.embedding_notice = Some(format!(
                    "The local embedding model could not load; using keyword retrieval: {error}"
                ));
            }
        }
    }

    fn install_embedder(&mut self, embedder: Embedder) {
        let shared: Arc<dyn Embed> = Arc::new(embedder);
        self.knowledge.set_embedder(shared);
        self.knowledge
            .set_query_embedding_cache_capacity(self.config.embeddings.query_cache_entries);
        self.embedding_notice = None;
        self.embedder_loaded = true;
    }

    fn embedding_coverage_notice(&self) -> Result<Option<String>> {
        let Some(embedder) = self.knowledge.embedder() else {
            return Ok(None);
        };
        let (total, current_model): (i64, i64) = self.knowledge.conn().query_row(
            "SELECT COUNT(m.chunk_rowid),
                    COUNT(CASE WHEN e.model_id = ?1 THEN e.chunk_rowid END)
             FROM knowledge_chunk_meta m
             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = m.chunk_rowid",
            params![embedder.model_id()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if total > current_model {
            Ok(Some(format!(
                "The active model has vectors for {current_model}/{total} indexed chunks; run `bpx-rag embed` to build or replace the remaining vectors."
            )))
        } else {
            Ok(None)
        }
    }
}

/// Load the configured embedder for the watcher. Failure means changed chunks are indexed without
/// vectors, and the reason is reported on stderr.
pub fn load_watch_embedder() -> Option<Arc<dyn Embed>> {
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[bpx-rag] watch: could not load config: {error:#}");
            return None;
        }
    };
    if !config.embeddings.enabled {
        return None;
    }
    let model_dir = model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        return None;
    }
    match Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
    ) {
        Ok(embedder) => Some(Arc::new(embedder)),
        Err(error) => {
            eprintln!("[bpx-rag] watch: embedding model could not load; indexing without vectors: {error:#}");
            None
        }
    }
}

/// One status line for files a watcher sync skipped because they are not valid UTF-8.
fn skipped_note(skipped: &[knowledge::sync::SkippedFile]) -> String {
    if skipped.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = skipped.iter().map(|file| file.rel_path.as_str()).collect();
    format!(
        ", skipped {} file(s) that are not valid UTF-8: {}",
        skipped.len(),
        names.join(", ")
    )
}

pub fn format_sync_result(result: &knowledge::sync::SyncResult) -> String {
    let summary = format!(
        "Source '{}': scanned {}, added {}, updated {}, removed {}, unchanged {}; {} sensitive files skipped by policy; {} chunks indexed this sync ({} total; {} Tree-sitter / {} existing chunker across {} / {} changed files) in {} ms.",
        result.source_label,
        result.files_scanned,
        result.files_added,
        result.files_updated,
        result.files_removed,
        result.files_unchanged,
        result.sensitive_files_skipped,
        result.chunks_indexed,
        result.chunks_total,
        result.tree_sitter_chunks_indexed,
        result.existing_chunker_chunks_indexed,
        result.tree_sitter_files_indexed,
        result.existing_chunker_files_indexed,
        result.duration_ms
    );
    result.skipped_files.iter().fold(summary, |text, skipped| {
        format!(
            "{text}\nwarning: skipped '{}' from this source: {}",
            skipped.rel_path, skipped.reason
        )
    })
}

pub fn model_directory(config: &Config) -> PathBuf {
    std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(&config.embeddings.model_dir).join(&config.embeddings.model)
        })
}

pub fn embedding_model_status(config: &Config) -> String {
    let path = model_directory(config);
    let device = match config.embeddings.device {
        crate::config::EmbeddingDevice::Cpu => "cpu".to_string(),
        crate::config::EmbeddingDevice::Cuda => {
            #[cfg(feature = "gpu-cuda")]
            {
                "cuda".to_string()
            }
            #[cfg(not(feature = "gpu-cuda"))]
            {
                "cuda requested, but this binary was built without `gpu-cuda`".to_string()
            }
        }
    };
    if embed::model_files_present(&path, &config.embeddings.model) {
        format!(
            "{} model files cached at {} (integrity is checked when loaded); device: {device}",
            config.embeddings.model,
            path.display()
        )
    } else {
        format!(
            "{} not installed at {}; device: {device}",
            config.embeddings.model,
            path.display()
        )
    }
}

fn embedding_notice(config: &Config) -> Option<String> {
    if !config.embeddings.enabled {
        return Some("Embeddings are disabled in config.".into());
    }
    let model_dir = model_directory(config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        return Some(format!(
            "Semantic retrieval is unavailable; install the local model with `bpx-rag models download` ({}).",
            model_dir.display()
        ));
    }
    None
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

fn validate_label(label: &str) -> Result<()> {
    if label.trim().is_empty() || label.len() > 256 {
        return Err(anyhow!("source label must be between 1 and 256 bytes"));
    }
    Ok(())
}

fn add_embedding_notice(mut text: String, notice: Option<&str>, count_only: bool) -> String {
    let Some(notice) = notice else {
        return text;
    };
    if count_only {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
            value["embedding_notice"] = json!(notice);
            return serde_json::to_string_pretty(&value).unwrap_or(text);
        }
    }
    text.push_str("\n\n**Embedding status:** ");
    text.push_str(notice);
    text
}

fn format_symbol_report(name: &str, mode: MatchMode, report: &SymbolReport) -> String {
    let mode_label = match mode {
        MatchMode::Exact => "exact",
        MatchMode::Prefix => "prefix",
    };
    let mut lines = Vec::new();
    if report.hits.is_empty() {
        lines.push(format!("No symbols matched `{name}` ({mode_label})."));
    } else {
        lines.push(format!(
            "Showing {} symbol match(es) for `{name}` ({mode_label}).",
            report.hits.len()
        ));
        for hit in &report.hits {
            let qualified = match &hit.container {
                Some(container) => format!("{container}::{}", hit.name),
                None => hit.name.clone(),
            };
            lines.push(format!(
                "{}:{}-{}  {}  {}  [source: {}]",
                hit.rel_path, hit.line_start, hit.line_end, hit.kind, qualified, hit.source_label
            ));
        }
        if report.truncated {
            lines.push(format!(
                "More matches exist. Raise the limit (maximum {MAX_SYMBOL_LIMIT}) to see them."
            ));
        }
    }
    for pending in &report.pending {
        lines.push(format!(
            "Symbol index pending for source '{}': {} file(s) not yet symbol-indexed. Run `bpx-rag sync --label {}` or bpx_sync with that label to backfill.",
            pending.source_label, pending.files, pending.source_label
        ));
    }
    lines.join("\n")
}
