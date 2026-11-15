use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use anyhow::{anyhow, Result};
use std::time::Instant;

pub(in super::super) struct LoadedModel {
    pub(in super::super) embedder: Embedder,
    pub(in super::super) model_id: String,
    pub(in super::super) embedding_dimension: usize,
    pub(in super::super) model_artifact_bytes: u64,
    pub(in super::super) model_load_ms: f64,
}

pub(in super::super) fn load_model(config: &Config) -> Result<LoadedModel> {
    let model_dir = crate::app::model_directory(config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        return Err(anyhow!(
            "evaluation requires the configured model to be cached; run `bpx-rag models download` first ({}).",
            model_dir.display()
        ));
    }

    let model_start = Instant::now();
    let embedder = Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
    )?;
    let model_load_ms = model_start.elapsed().as_secs_f64() * 1000.0;
    let model_id = embedder.model_id().to_string();
    let embedding_dimension = embedder.dim();
    let model_artifact_bytes = embed::model_cache_bytes(&model_dir, &config.embeddings.model)?;
    Ok(LoadedModel {
        embedder,
        model_id,
        embedding_dimension,
        model_artifact_bytes,
        model_load_ms,
    })
}
