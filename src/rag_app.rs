use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use rusqlite::params;
use serde_json::json;

use crate::config::Config;
use crate::embedder::{self, Embed, Embedder};
use crate::knowledge::{self, KnowledgeSourceInfo, KnowledgeStore};
use crate::retrieval::{self, QueryRequest};
use crate::search_model::SearchProfile;
use crate::session::SessionStore;
use crate::store::ContentStore;

pub struct RagApp {
    config: Config,
    content: ContentStore,
    session: SessionStore,
    knowledge: KnowledgeStore,
    embedding_notice: Option<String>,
    embedder_loaded: bool,
}

#[derive(Debug, Clone)]
pub struct SearchOptions<'a> {
    pub source: Option<&'a str>,
    pub filter: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub limit: u32,
    pub profile: Option<&'a str>,
    pub explain: bool,
    pub count_only: bool,
    pub use_memory: bool,
}

impl Default for SearchOptions<'_> {
    fn default() -> Self {
        Self {
            source: None,
            filter: None,
            content_type: None,
            limit: 10,
            profile: None,
            explain: false,
            count_only: false,
            use_memory: false,
        }
    }
}

impl RagApp {
    pub fn open(project_dir: &Path) -> Result<Self> {
        let config = Config::load()?;
        let content = ContentStore::open(project_dir)?;
        let session = SessionStore::open(project_dir)?;
        let knowledge = KnowledgeStore::open()?;
        let embedding_notice = embedding_notice(&config);
        Ok(Self {
            config,
            content,
            session,
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
        let outcome = retrieval::run_query(
            QueryRequest {
                query,
                source: options.source,
                content_type: options.content_type,
                knowledge_filter: options.filter,
                limit: options.limit.clamp(1, 50),
                snippet_bytes: self.config.search.snippet_bytes,
                profile,
                explain: options.explain,
                use_memory: options.use_memory,
                include_knowledge: true,
            },
            &self.config,
            &self.content,
            self.session.conn(),
            Some(&self.knowledge),
            options.count_only,
        )?;
        Ok(add_embedding_notice(
            outcome.text,
            self.embedding_notice.as_deref(),
            options.count_only,
        ))
    }

    pub fn add_source(
        &mut self,
        path: &Path,
        label: &str,
        glob: Option<&str>,
        enrichments: &[String],
    ) -> Result<String> {
        validate_label(label)?;
        if let Some(pattern) = glob {
            glob::Pattern::new(pattern).context("invalid source glob pattern")?;
        }
        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("could not resolve source path {}", path.display()))?;
        if !canonical_path.is_dir() {
            return Err(anyhow!("source path must be a directory"));
        }
        if self.knowledge.get_source(label)?.is_some() {
            return Err(anyhow!("a knowledge source named '{label}' already exists"));
        }
        self.ensure_embedder();
        self.knowledge
            .add_source(label, &canonical_path.to_string_lossy(), glob, enrichments)?;
        let source = self
            .knowledge
            .get_source(label)?
            .ok_or_else(|| anyhow!("new source could not be loaded"))?;
        let result = self.sync_one(&source)?;
        Ok(format_sync_result(&result))
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
        self.ensure_embedder();
        sources.iter().map(|source| self.sync_one(source)).collect()
    }

    pub fn remove_source(&self, label: &str) -> Result<knowledge::RemoveResult> {
        validate_label(label)?;
        self.knowledge.remove_source(label)
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
            return Ok("No sources are registered. Add one with `bpx-rag knowledge add`.".into());
        }
        let mut output = String::from("# Knowledge sources\n\n");
        for source in sources {
            let model_id = embedder::canonical_model_id(&self.config.embeddings.model)?;
            let (embedded, total): (i64, i64) = self.knowledge.conn().query_row(
                "SELECT COUNT(CASE WHEN e.model_id = ?1 THEN e.chunk_rowid END), COUNT(m.chunk_rowid)
                 FROM knowledge_chunk_meta m
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = m.chunk_rowid
                 WHERE m.source_id = ?2",
                params![model_id, source.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            output.push_str(&format!(
                "- **{}** — {} files, {} chunks, {}/{} embedded; last sync: {}\n  Path: `{}`\n",
                source.label,
                source.file_count,
                source.chunk_count,
                embedded,
                total,
                source.last_sync.as_deref().unwrap_or("never"),
                source.path
            ));
        }
        output.push_str(&format!(
            "\nEmbedding model: `{}` ({})\n",
            self.config.embeddings.model,
            self.embedding_model_status()
        ));
        Ok(output)
    }

    pub fn backfill_embeddings(&mut self) -> Result<usize> {
        self.ensure_embedder();
        let embedder = self.knowledge.embedder().ok_or_else(|| {
            anyhow!("embedding model is unavailable; run `bpx-rag models download` first")
        })?;
        let conn = self.knowledge.conn();
        let batch_size = self.config.embeddings.batch_size.clamp(1, 256);
        let mut total = 0usize;
        loop {
            let batch = {
                let mut statement = conn.prepare(
                    "SELECT kc.rowid, kc.content
                     FROM knowledge_chunks kc
                     JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                     LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                     WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1
                     ORDER BY kc.rowid
                     LIMIT ?2",
                )?;
                let rows = statement
                    .query_map(params![embedder.model_id(), batch_size as i64], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            if batch.is_empty() {
                break;
            }
            let texts = batch
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>();
            let vectors = embedder.embed_batch(&texts)?;
            if vectors.len() != batch.len() {
                return Err(anyhow!(
                    "embedding provider returned an unexpected batch size"
                ));
            }
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let transaction_result = (|| -> Result<()> {
                for ((rowid, _), vector) in batch.iter().zip(vectors.iter()) {
                    let vector = embedder::normalize_embedding(vector, embedder.dim())?;
                    conn.execute(
                        "INSERT OR REPLACE INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            *rowid,
                            embedder::embedding_to_bytes(&vector),
                            embedder.dim() as i64,
                            embedder.model_id()
                        ],
                    )?;
                }
                Ok(())
            })();
            match transaction_result {
                Ok(()) => conn.execute_batch("COMMIT")?,
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
            total += batch.len();
        }
        Ok(total)
    }

    pub fn embedding_model_status(&self) -> String {
        let model_dir = model_directory(&self.config);
        if embedder::model_files_present(&model_dir, &self.config.embeddings.model) {
            format!(
                "{} model files cached at {} (integrity is checked when loaded)",
                self.config.embeddings.model,
                model_dir.display()
            )
        } else {
            format!(
                "{} is not installed at {}; run `bpx-rag models download`",
                self.config.embeddings.model,
                model_dir.display()
            )
        }
    }

    pub fn download_embedding_model(&mut self) -> Result<String> {
        let model_dir = model_directory(&self.config);
        let embedder = Embedder::new(
            &model_dir,
            &self.config.embeddings.model,
            self.config.embeddings.batch_size,
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
        if !embedder::model_files_present(&model_dir, &self.config.embeddings.model) {
            self.embedding_notice = Some(format!(
                "Semantic retrieval is unavailable; install the local model with `bpx-rag models download` ({}).",
                model_dir.display()
            ));
            return;
        }
        match Embedder::load_cached(
            &model_dir,
            &self.config.embeddings.model,
            self.config.embeddings.batch_size,
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
        self.content.set_embedder(Arc::clone(&shared));
        self.knowledge.set_embedder(shared);
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
                "The active model has vectors for {current_model}/{total} indexed chunks; run `bpx-rag knowledge embed-backfill` to build or replace the remaining vectors."
            )))
        } else {
            Ok(None)
        }
    }
}

pub fn format_sync_result(result: &knowledge::sync::SyncResult) -> String {
    format!(
        "Source '{}': scanned {}, added {}, updated {}, removed {}, unchanged {}; {} chunks indexed in {} ms.",
        result.source_label,
        result.files_scanned,
        result.files_added,
        result.files_updated,
        result.files_removed,
        result.files_unchanged,
        result.chunks_total,
        result.duration_ms
    )
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
    if embedder::model_files_present(&path, &config.embeddings.model) {
        format!(
            "{} model files cached at {} (integrity is checked when loaded)",
            config.embeddings.model,
            path.display()
        )
    } else {
        format!(
            "{} not installed at {}",
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
    if !embedder::model_files_present(&model_dir, &config.embeddings.model) {
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
