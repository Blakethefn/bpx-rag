use super::*;

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

fn mock_embedding(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];
    for (i, byte) in text.bytes().enumerate() {
        vec[i % dim] += byte as f32;
    }
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
