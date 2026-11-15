use std::sync::Arc;

use crate::config::Config;
use crate::embed::{self, Embed, Embedder};

use super::model_directory;

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
