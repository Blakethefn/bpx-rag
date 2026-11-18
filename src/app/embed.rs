use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::config::{Config, EmbeddingDevice};
use crate::embed::{self, Embed, Embedder};

use super::RagApp;

mod coverage;
mod model;
mod runtime;

pub use model::{embedding_model_status, load_watch_embedder};
#[cfg(test)]
pub(in crate::app) use runtime::{gpu_skip_reason_for, load_with_fallback};
pub(in crate::app) use runtime::{
    initialize_once, load_cached_embedder, ready_state, EmbedderState,
};

impl RagApp {
    pub(in crate::app) fn ensure_embedder(&mut self) {
        let mut state = self
            .embedder_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*state {
            EmbedderState::GpuReady | EmbedderState::CpuReady => return,
            EmbedderState::Unavailable(notice) => {
                self.embedding_notice = Some(notice.clone());
                return;
            }
            EmbedderState::Uninitialized => {}
        }

        let initialized = initialize_once(&mut state, || {
            self.load_configured_embedder()
                .map_err(|error| error.to_string())
        });
        drop(state);
        match initialized {
            Ok(Some((embedder, device))) => self.install_embedder(embedder, device),
            Ok(None) => {}
            Err(error) => self.embedding_notice = Some(error),
        }
    }

    fn load_configured_embedder(&self) -> Result<(Embedder, EmbeddingDevice)> {
        if !self.config.embeddings.enabled {
            return Err(anyhow!("Embeddings are disabled in config."));
        }
        let model_dir = model_directory(&self.config);
        if !embed::model_files_present(&model_dir, &self.config.embeddings.model) {
            return Err(anyhow!(
                "Semantic retrieval is unavailable; install the local model with `bpx-rag models download` ({}).",
                model_dir.display()
            ));
        }
        load_cached_embedder(&self.config, &model_dir)
    }

    pub(in crate::app) fn install_embedder(&mut self, embedder: Embedder, device: EmbeddingDevice) {
        let shared: Arc<dyn Embed> = Arc::new(embedder);
        self.knowledge.set_embedder(shared);
        self.knowledge
            .set_query_embedding_cache_capacity(self.config.embeddings.query_cache_entries);
        self.embedding_notice = None;
        *self
            .embedder_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = ready_state(device);
    }

    pub fn download_embedding_model(&mut self) -> Result<String> {
        let model_dir = model_directory(&self.config);
        Embedder::download_model(&model_dir, &self.config.embeddings.model)?;
        let (embedder, device) = load_cached_embedder(&self.config, &model_dir)?;
        self.install_embedder(embedder, device);
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

pub fn model_directory(config: &Config) -> PathBuf {
    std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(&config.embeddings.model_dir).join(&config.embeddings.model)
        })
}

#[cfg(test)]
mod tests;

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
