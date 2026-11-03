//! ONNX cross-encoder adapter, built on `fastembed`'s reranker. It runs on the same `ort` runtime as
//! the embedder, so no second inference runtime is added. Loading never touches the network.

use super::Rerank;
use crate::config::EmbeddingDevice;
use anyhow::{anyhow, Context, Result};
use fastembed::{
    OnnxSource, RerankInitOptionsUserDefined, TextRerank, TokenizerFiles, UserDefinedRerankingModel,
};
use std::path::Path;
use std::sync::Mutex;

/// The model graph that must sit in the reranker's model directory.
pub const MODEL_FILE: &str = "model.onnx";
/// Tokenizer files that must sit next to the model graph.
pub const TOKENIZER_FILES: [&str; 4] = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];
/// Longest (query, passage) pair the model reads. A cross-encoder reads both texts together, so a
/// long passage is cut at this limit.
const MAX_LENGTH: usize = 512;
const BATCH_SIZE: usize = 16;

/// Files the reranker needs that are absent from `model_dir`.
pub fn missing_files(model_dir: &Path) -> Vec<&'static str> {
    std::iter::once(MODEL_FILE)
        .chain(TOKENIZER_FILES)
        .filter(|name| !model_dir.join(name).is_file())
        .collect()
}

/// A cross-encoder loaded from a complete local model directory.
pub struct OnnxCrossEncoder {
    inner: Mutex<TextRerank>,
}

impl OnnxCrossEncoder {
    /// Load a complete local model directory. Fails without a network request when files are missing.
    pub fn load_cached(
        model_dir: &Path,
        device: EmbeddingDevice,
        intra_threads: usize,
    ) -> Result<Self> {
        let missing = missing_files(model_dir);
        if !missing.is_empty() {
            anyhow::bail!(
                "reranker model files are missing in {}: {}",
                model_dir.display(),
                missing.join(", ")
            );
        }
        let read = |name: &str| -> Result<Vec<u8>> {
            std::fs::read(model_dir.join(name)).with_context(|| format!("failed to read {name}"))
        };
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        };
        let model = UserDefinedRerankingModel::new(
            OnnxSource::File(model_dir.join(MODEL_FILE)),
            tokenizer_files,
        );
        let options = RerankInitOptionsUserDefined::new()
            .with_environment(crate::embed::shared_ort_environment()?)
            .with_max_length(MAX_LENGTH)
            .with_intra_threads(intra_threads.clamp(1, 256));
        let options = match device {
            EmbeddingDevice::Cpu => options,
            EmbeddingDevice::Cuda => {
                #[cfg(feature = "gpu-cuda")]
                {
                    options.with_execution_providers(vec![ort::ep::CUDA::default()
                        .build()
                        .error_on_failure()])
                }
                #[cfg(not(feature = "gpu-cuda"))]
                {
                    anyhow::bail!(
                        "CUDA was selected, but this binary lacks CUDA support; rebuild with `--features gpu-cuda`"
                    )
                }
            }
        };
        let inner = TextRerank::try_new_from_user_defined(model, options)
            .map_err(|error| anyhow!("failed to load reranker model: {error}"))?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }
}

impl Rerank for OnnxCrossEncoder {
    fn score(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        let mut model = self
            .inner
            .lock()
            .map_err(|_| anyhow!("reranker lock was poisoned"))?;
        let results = model
            .rerank(query, passages, false, Some(BATCH_SIZE))
            .map_err(|error| anyhow!("reranking failed: {error}"))?;
        let mut scores: Vec<Option<f32>> = vec![None; passages.len()];
        for result in results {
            let slot = scores.get_mut(result.index).ok_or_else(|| {
                anyhow!("reranker returned an out-of-range index {}", result.index)
            })?;
            *slot = Some(result.score);
        }
        scores
            .into_iter()
            .enumerate()
            .map(|(index, score)| {
                score.ok_or_else(|| anyhow!("reranker returned no score for passage {index}"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_lists_every_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(missing_files(dir.path()).len(), 5);
    }

    #[test]
    fn missing_files_is_empty_when_all_files_are_present() {
        let dir = tempfile::tempdir().unwrap();
        for name in std::iter::once(MODEL_FILE).chain(TOKENIZER_FILES) {
            std::fs::write(dir.path().join(name), b"{}").unwrap();
        }
        assert!(missing_files(dir.path()).is_empty());
    }

    #[test]
    fn load_rejects_an_unreadable_model_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        for name in std::iter::once(MODEL_FILE).chain(TOKENIZER_FILES) {
            std::fs::write(dir.path().join(name), b"not a real model").unwrap();
        }
        let result = OnnxCrossEncoder::load_cached(dir.path(), EmbeddingDevice::Cpu, 1);
        assert!(result.is_err());
    }
}
