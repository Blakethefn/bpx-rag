use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use fastembed::{TextEmbedding, TextInitOptions};

use super::model::{max_length_for, model_files_present, resolve_model, shared_ort_environment};
use crate::config::EmbeddingDevice;

impl super::Embedder {
    pub fn load_cached_with_device(
        model_dir: &Path,
        model_name: &str,
        batch_size: usize,
        intra_threads: usize,
        device: EmbeddingDevice,
    ) -> Result<Self> {
        let model = resolve_model(model_name)?;
        let model_info = TextEmbedding::get_model_info(&model)
            .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
        let model_id = model_info.model_code.clone();
        let dim = model_info.dim;
        if !model_files_present(model_dir, model_name) {
            anyhow::bail!(
                "embedding model '{}' is not completely cached under {}; run `bpx-rag models download`",
                model_info.model_code,
                model_dir.display()
            );
        }
        let max_length = max_length_for(&model);
        let options = TextInitOptions::new(model)
            .with_cache_dir(model_dir.to_path_buf())
            .with_max_length(max_length)
            .with_intra_threads(intra_threads.clamp(1, 256))
            .with_environment(shared_ort_environment()?)
            .with_show_download_progress(false);
        let options = match device {
            EmbeddingDevice::Cpu => options,
            EmbeddingDevice::Cuda => {
                #[cfg(feature = "gpu-cuda")]
                {
                    options.with_execution_providers(vec![ort::ep::CUDA::default()
                        .with_conv_algorithm_search(ort::ep::cuda::ConvAlgorithmSearch::Default)
                        .build()
                        .error_on_failure()])
                }
                #[cfg(not(feature = "gpu-cuda"))]
                {
                    anyhow::bail!(
                        "CUDA was selected, but this binary lacks CUDA support; rebuild with `--features gpu-cuda`"
                    );
                }
            }
        };
        let inner = TextEmbedding::try_new(options)
            .map_err(|error| anyhow!("failed to load cached embedding model: {error}"))?;
        Ok(Self {
            inner: Mutex::new(inner),
            model_id,
            dim,
            batch_size: batch_size.clamp(1, 256),
        })
    }
}
