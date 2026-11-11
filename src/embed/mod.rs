use crate::config::EmbeddingDevice;
use anyhow::{anyhow, Result};
use fastembed::{TextEmbedding, TextInitOptions};
use std::path::Path;
use std::sync::Mutex;

pub mod cache;

/// The legacy MiniLM dimension used by the deterministic mock embedder.
#[cfg(test)]
pub const EMBEDDING_DIM: usize = 384;
pub const DEFAULT_MODEL: &str = "all-MiniLM-L6-v2";
pub const CODE_MODEL: &str = "jina-embeddings-v2-base-code";
const MINI_LM_MAX_LENGTH: usize = 256;
// Source chunks are bounded to 4 KiB. A 1,024-token window covers those chunks
// while avoiding the latency and memory cost of processing the model's full
// 8K-token capability for unusually large chunks.
const JINA_CODE_MAX_LENGTH: usize = 1024;
mod model;

pub use model::{canonical_model_id, model_cache_bytes, model_files_present};
use model::{max_length_for, resolve_model, shared_ort_environment};

/// Trait for embedding text into vectors. Enables mocking in tests.
#[allow(dead_code)]
pub trait Embed: Send + Sync {
    /// Embed a single text, returning an L2-normalized vector.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed multiple texts in batches, returning L2-normalized vectors.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// Maximum input texts to include in one embedding request.
    fn max_batch_size(&self) -> usize {
        16
    }

    /// The dimensionality of the output vectors.
    fn dim(&self) -> usize;

    /// Canonical upstream model identity. Vectors from different identities
    /// must never be compared or fused.
    fn model_id(&self) -> &str {
        "sentence-transformers/all-MiniLM-L6-v2"
    }
}

/// Production ONNX embedder. Model download is separate from cached loading so
/// query and indexing paths can stay offline unless files are already present.
pub struct Embedder {
    inner: Mutex<TextEmbedding>,
    model_id: String,
    dim: usize,
    batch_size: usize,
}

impl Embedder {
    /// Explicitly download the selected model, then load it from local cache.
    pub fn new_with_device(
        model_dir: &Path,
        model_name: &str,
        batch_size: usize,
        intra_threads: usize,
        device: EmbeddingDevice,
    ) -> Result<Self> {
        Self::download_model(model_dir, model_name)?;
        Self::load_cached_with_device(model_dir, model_name, batch_size, intra_threads, device)
    }

    /// Load only a complete local cache entry. This never attempts a network request.
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
                    // Heuristic cuDNN kernel choice reduces run-to-run variation in embeddings; the
                    // default exhaustive search benchmarks kernels per run and can flip near-ties.
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

/// Validate vector shape and values, then normalize to unit length so cosine
/// similarity is represented consistently as a dot product in SQLite.
pub fn normalize_embedding(embedding: &[f32], expected_dim: usize) -> Result<Vec<f32>> {
    if embedding.len() != expected_dim {
        anyhow::bail!(
            "embedding dimension mismatch: expected {expected_dim}, received {}",
            embedding.len()
        );
    }
    if embedding.iter().any(|value| !value.is_finite()) {
        anyhow::bail!("embedding contains a non-finite value");
    }
    let norm = embedding
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        anyhow::bail!("embedding has zero magnitude");
    }
    Ok(embedding
        .iter()
        .map(|value| (f64::from(*value) / norm) as f32)
        .collect())
}

impl Embed for Embedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut embeddings = self.embed_batch(&[text])?;
        embeddings
            .pop()
            .ok_or_else(|| anyhow!("embedding provider returned no vector"))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("embedding model lock was poisoned"))?;
        let mut embeddings = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.batch_size) {
            // FastEmbed retains raw ONNX outputs for every internal batch until
            // `embed` finishes. Keep each call to one configured batch so a
            // large source cannot accumulate corpus-wide activation tensors.
            let batch_embeddings = inner
                .embed(batch, Some(self.batch_size))
                .map_err(|error| anyhow!("embedding generation failed: {error}"))?;
            embeddings.extend(batch_embeddings);
        }
        Ok(embeddings)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn max_batch_size(&self) -> usize {
        self.batch_size
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

/// Convert a f32 embedding vector to raw bytes for SQLite BLOB storage.
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Convert raw bytes from SQLite BLOB back to a f32 vector.
#[allow(dead_code)]
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
