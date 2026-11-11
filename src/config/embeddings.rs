use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingDevice {
    #[default]
    Cpu,
    Cuda,
}

impl EmbeddingDevice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    /// Model alias: all-MiniLM-L6-v2 or jina-embeddings-v2-base-code.
    pub model: String,
    /// Directory for storing downloaded models
    pub model_dir: String,
    /// Batch size for embedding generation (default 16)
    pub batch_size: usize,
    /// ONNX Runtime intra-op CPU threads (default: available logical CPUs).
    #[serde(default = "default_embedding_threads")]
    pub intra_threads: usize,
    /// Inference provider. CUDA requires a `gpu-cuda` build and compatible runtime libraries.
    pub device: EmbeddingDevice,
    /// Enable/disable embedding generation (default true)
    pub enabled: bool,
    /// Query embeddings kept in memory for repeated searches. 0 disables the cache (default 256).
    #[serde(default = "default_query_cache_entries")]
    pub query_cache_entries: usize,
}

fn default_query_cache_entries() -> usize {
    crate::embed::cache::DEFAULT_CAPACITY
}

fn default_embedding_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 256)
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        let model_dir = dirs::data_local_dir()
            .map(|d| {
                d.join(crate::db::APP_NAME)
                    .join("models")
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "~/.local/share/bpx-rag/models".to_string());

        Self {
            model: "jina-embeddings-v2-base-code".to_string(),
            model_dir,
            batch_size: 16,
            intra_threads: default_embedding_threads(),
            device: EmbeddingDevice::Cpu,
            enabled: true,
            query_cache_entries: default_query_cache_entries(),
        }
    }
}
