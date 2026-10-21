pub mod model;
pub mod pool;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use model::CandleEmbedder;

/// The embedding dimension for all-MiniLM-L6-v2.
pub const EMBEDDING_DIM: usize = model::EMBEDDING_DIM;

/// Trait for embedding text into vectors. Enables mocking in tests.
#[allow(dead_code)]
pub trait Embed: Send + Sync {
    /// Embed a single text, returning an L2-normalized vector.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed multiple texts in batches, returning L2-normalized vectors.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// The dimensionality of the output vectors.
    fn dim(&self) -> usize;
}

/// Production embedder backed by Candle + all-MiniLM-L6-v2.
///
/// Lazily loads the model on first use. Downloads via hf-hub if not cached.
pub struct Embedder {
    inner: CandleEmbedder,
}

impl Embedder {
    /// Load a model from the given directory, downloading missing files first.
    /// Use `load_cached` for query paths that must never access the network.
    pub fn new(model_dir: &Path) -> Result<Self> {
        Self::download_model(model_dir)?;
        Self::load_cached(model_dir)
    }

    /// Load only local model files. This never attempts a network request.
    pub fn load_cached(model_dir: &Path) -> Result<Self> {
        if !model_files_present(model_dir) {
            anyhow::bail!(
                "embedding model is not installed at {}; run `bpx-rag models download`",
                model_dir.display()
            );
        }
        let inner = CandleEmbedder::load(model_dir).context("failed to load embedding model")?;
        Ok(Self { inner })
    }

    /// Download missing model files to the configured cache directory.
    pub fn download_model(model_dir: &Path) -> Result<PathBuf> {
        ensure_model_downloaded(model_dir)
    }

    /// Default model directory: `~/.local/share/bpcontext/models/all-MiniLM-L6-v2`
    #[allow(dead_code)]
    pub fn default_model_dir() -> Result<PathBuf> {
        let data_dir =
            dirs::data_local_dir().context("could not determine local data directory")?;
        Ok(data_dir
            .join("bpcontext")
            .join("models")
            .join("all-MiniLM-L6-v2"))
    }
}

pub fn model_files_present(model_dir: &Path) -> bool {
    ["config.json", "model.safetensors", "tokenizer.json"]
        .iter()
        .all(|file| model_dir.join(file).is_file())
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
        self.inner.embed_one(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }

    fn dim(&self) -> usize {
        EMBEDDING_DIM
    }
}

/// Ensure model files exist in `model_dir`, downloading via hf-hub if needed.
///
/// Returns the path where the model files are located.
fn ensure_model_downloaded(model_dir: &Path) -> Result<PathBuf> {
    let config_path = model_dir.join("config.json");
    let weights_path = model_dir.join("model.safetensors");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if config_path.exists() && weights_path.exists() && tokenizer_path.exists() {
        return Ok(model_dir.to_path_buf());
    }

    eprintln!("[bpx-rag] Downloading all-MiniLM-L6-v2 model (~80MB, one-time)...");

    let api = hf_hub::api::sync::Api::new().context("failed to initialize hf-hub API")?;
    let repo = api.model("sentence-transformers/all-MiniLM-L6-v2".to_string());

    // Download each required file
    let files = ["config.json", "model.safetensors", "tokenizer.json"];
    for filename in &files {
        let downloaded_path = repo
            .get(filename)
            .with_context(|| format!("failed to download {filename} from HuggingFace"))?;

        let target = model_dir.join(filename);
        if downloaded_path != target {
            // hf-hub caches files in its own location — copy to our model_dir
            std::fs::create_dir_all(model_dir)
                .with_context(|| format!("failed to create {}", model_dir.display()))?;
            std::fs::copy(&downloaded_path, &target)
                .with_context(|| format!("failed to copy {filename} to model dir"))?;
        }
    }

    eprintln!("[bpx-rag] Model downloaded successfully.");
    Ok(model_dir.to_path_buf())
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
pub(crate) mod tests {
    use super::*;

    /// Mock embedder for unit tests — returns deterministic vectors.
    pub(crate) struct MockEmbedder {
        pub dim: usize,
    }

    impl MockEmbedder {
        pub fn new() -> Self {
            Self { dim: EMBEDDING_DIM }
        }
    }

    impl Embed for MockEmbedder {
        fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
            Ok(mock_embedding(text, self.dim))
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|t| mock_embedding(t, self.dim)).collect())
        }

        fn dim(&self) -> usize {
            self.dim
        }
    }

    /// Generate a deterministic mock embedding from text.
    /// Uses a simple hash-based approach so similar inputs get different vectors.
    fn mock_embedding(text: &str, dim: usize) -> Vec<f32> {
        let mut vec = vec![0.0f32; dim];
        for (i, byte) in text.bytes().enumerate() {
            vec[i % dim] += byte as f32;
        }
        // L2 normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        for val in &mut vec {
            *val /= norm;
        }
        vec
    }

    #[test]
    fn embedding_bytes_roundtrip() {
        let original = vec![1.0f32, -2.5, 3.125, 0.0, -0.001];
        let bytes = embedding_to_bytes(&original);
        let recovered = bytes_to_embedding(&bytes);

        assert_eq!(original.len(), recovered.len());
        for (a, b) in original.iter().zip(recovered.iter()) {
            assert!((a - b).abs() < 1e-7);
        }
    }

    #[test]
    fn normalize_embedding_rejects_invalid_vectors_and_normalizes_valid_ones() {
        let normalized = normalize_embedding(&[3.0, 4.0], 2).unwrap();
        assert!((normalized[0] - 0.6).abs() < 1e-6);
        assert!((normalized[1] - 0.8).abs() < 1e-6);
        assert!(normalize_embedding(&[1.0], 2).is_err());
        assert!(normalize_embedding(&[f32::NAN, 0.0], 2).is_err());
        assert!(normalize_embedding(&[0.0, 0.0], 2).is_err());
    }

    #[test]
    fn mock_embedder_returns_correct_dim() {
        let embedder = MockEmbedder::new();
        let vec = embedder.embed_one("hello world").unwrap();
        assert_eq!(vec.len(), EMBEDDING_DIM);
    }

    #[test]
    fn mock_embedder_returns_normalized_vectors() {
        let embedder = MockEmbedder::new();
        let vec = embedder.embed_one("test input").unwrap();
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mock_embedder_batch_matches_individual() {
        let embedder = MockEmbedder::new();
        let texts = &["hello", "world"];
        let batch = embedder.embed_batch(texts).unwrap();
        let individual: Vec<Vec<f32>> = texts
            .iter()
            .map(|t| embedder.embed_one(t).unwrap())
            .collect();

        assert_eq!(batch, individual);
    }
}
