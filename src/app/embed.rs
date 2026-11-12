use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::params;
use serde_json::json;

use crate::config::Config;
use crate::embed::{self, Embed, Embedder};

use super::RagApp;

impl RagApp {
    pub(in crate::app) fn ensure_embedder(&mut self) {
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

    pub(in crate::app) fn install_embedder(&mut self, embedder: Embedder) {
        let shared: Arc<dyn Embed> = Arc::new(embedder);
        self.knowledge.set_embedder(shared);
        self.knowledge
            .set_query_embedding_cache_capacity(self.config.embeddings.query_cache_entries);
        self.embedding_notice = None;
        self.embedder_loaded = true;
    }

    pub(super) fn embedding_coverage_notice(&self) -> Result<Option<String>> {
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

    pub fn embedding_model_status(&self) -> String {
        embedding_model_status(&self.config)
    }
}

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

pub(super) fn embedding_notice(config: &Config) -> Option<String> {
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

pub(super) fn add_embedding_notice(
    mut text: String,
    notice: Option<&str>,
    count_only: bool,
) -> String {
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
