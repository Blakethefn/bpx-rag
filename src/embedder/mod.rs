use anyhow::{anyhow, Context, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use hf_hub::{api::sync::ApiBuilder, Cache};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
const TOKENIZER_FILES: [&str; 4] = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

/// Trait for embedding text into vectors. Enables mocking in tests.
#[allow(dead_code)]
pub trait Embed: Send + Sync {
    /// Embed a single text, returning an L2-normalized vector.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed multiple texts in batches, returning L2-normalized vectors.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

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
    pub fn new(
        model_dir: &Path,
        model_name: &str,
        batch_size: usize,
        intra_threads: usize,
    ) -> Result<Self> {
        Self::download_model(model_dir, model_name)?;
        Self::load_cached(model_dir, model_name, batch_size, intra_threads)
    }

    /// Load only a complete local cache entry. This never attempts a network request.
    pub fn load_cached(
        model_dir: &Path,
        model_name: &str,
        batch_size: usize,
        intra_threads: usize,
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
        let inner = TextEmbedding::try_new(options)
            .map_err(|error| anyhow!("failed to load cached embedding model: {error}"))?;
        Ok(Self {
            inner: Mutex::new(inner),
            model_id,
            dim,
            batch_size: batch_size.clamp(1, 256),
        })
    }

    /// Download the selected model artifacts explicitly into the local HF cache.
    pub fn download_model(model_dir: &Path, model_name: &str) -> Result<PathBuf> {
        let model = resolve_model(model_name)?;
        let model_info = TextEmbedding::get_model_info(&model)
            .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
        let cache_dir = model_dir.to_path_buf();
        let api = ApiBuilder::new()
            .with_cache_dir(cache_dir.clone())
            .with_progress(true)
            .build()
            .context("failed to initialize Hugging Face cache/API")?;
        let repo = api.model(model_info.model_code.clone());
        let files = std::iter::once(model_info.model_file.as_str())
            .chain(TOKENIZER_FILES.iter().copied())
            .chain(model_info.additional_files.iter().map(String::as_str));
        for file in files {
            repo.get(file).with_context(|| {
                format!("failed to download {file} for {}", model_info.model_code)
            })?;
        }
        Ok(cache_dir)
    }
}

fn shared_ort_environment() -> Result<ort::environment::Environment> {
    static ENVIRONMENT: OnceLock<Mutex<Option<ort::environment::Environment>>> = OnceLock::new();
    let environment = ENVIRONMENT.get_or_init(|| Mutex::new(None));
    let mut environment = environment
        .lock()
        .map_err(|_| anyhow!("ONNX Runtime environment lock was poisoned"))?;
    if environment.is_none() {
        *environment = Some(
            ort::init()
                .with_name("bpx-rag")
                .build()
                .map_err(|error| anyhow!("failed to initialize ONNX Runtime: {error}"))?,
        );
    }
    environment
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow!("ONNX Runtime environment initialization failed"))
}

pub fn model_files_present(model_dir: &Path, model_name: &str) -> bool {
    let Ok(model) = resolve_model(model_name) else {
        return false;
    };
    let Ok(model_info) = TextEmbedding::get_model_info(&model) else {
        return false;
    };
    let repo = Cache::new(model_dir.to_path_buf()).model(model_info.model_code.clone());
    std::iter::once(model_info.model_file.as_str())
        .chain(TOKENIZER_FILES.iter().copied())
        .chain(model_info.additional_files.iter().map(String::as_str))
        .all(|file| repo.get(file).is_some())
}

pub fn canonical_model_id(model_name: &str) -> Result<String> {
    let model = resolve_model(model_name)?;
    let info = TextEmbedding::get_model_info(&model)
        .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
    Ok(info.model_code.clone())
}

pub fn model_cache_bytes(model_dir: &Path, model_name: &str) -> Result<u64> {
    let model = resolve_model(model_name)?;
    let info = TextEmbedding::get_model_info(&model)
        .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
    let repo = Cache::new(model_dir.to_path_buf()).model(info.model_code.clone());
    std::iter::once(info.model_file.as_str())
        .chain(TOKENIZER_FILES.iter().copied())
        .chain(info.additional_files.iter().map(String::as_str))
        .try_fold(0_u64, |total, file| {
            let path = repo
                .get(file)
                .ok_or_else(|| anyhow!("cached model artifact is missing: {file}"))?;
            Ok(total + std::fs::metadata(path)?.len())
        })
}

fn resolve_model(model_name: &str) -> Result<EmbeddingModel> {
    match model_name {
        DEFAULT_MODEL | "sentence-transformers/all-MiniLM-L6-v2" => {
            Ok(EmbeddingModel::AllMiniLML6V2)
        }
        CODE_MODEL | "jinaai/jina-embeddings-v2-base-code" => {
            Ok(EmbeddingModel::JinaEmbeddingsV2BaseCode)
        }
        other => anyhow::bail!(
            "unsupported embedding model '{other}'; choose '{DEFAULT_MODEL}' or '{CODE_MODEL}'"
        ),
    }
}

fn max_length_for(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::JinaEmbeddingsV2BaseCode => JINA_CODE_MAX_LENGTH,
        _ => MINI_LM_MAX_LENGTH,
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
        inner
            .embed(texts, Some(self.batch_size))
            .map_err(|error| anyhow!("embedding generation failed: {error}"))
    }

    fn dim(&self) -> usize {
        self.dim
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
    fn configured_models_have_distinct_canonical_ids_and_missing_cache_is_offline() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            canonical_model_id(DEFAULT_MODEL).unwrap(),
            "Qdrant/all-MiniLM-L6-v2-onnx"
        );
        assert_eq!(
            canonical_model_id(CODE_MODEL).unwrap(),
            "jinaai/jina-embeddings-v2-base-code"
        );
        assert_ne!(
            canonical_model_id(DEFAULT_MODEL).unwrap(),
            canonical_model_id(CODE_MODEL).unwrap()
        );
        assert!(!model_files_present(dir.path(), DEFAULT_MODEL));
        assert!(!model_files_present(dir.path(), CODE_MODEL));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(canonical_model_id("unknown-model").is_err());
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
