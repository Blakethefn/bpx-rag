//! ONNX cross-encoder adapter, built on `fastembed`'s reranker. It runs on the same `ort` runtime as
//! the embedder, so no second inference runtime is added. Loading never touches the network.

use super::{install, Rerank};
use crate::config::EmbeddingDevice;
use anyhow::{anyhow, Context, Result};
use fastembed::{
    OnnxSource, RerankInitOptionsUserDefined, TextRerank, TokenizerFiles, UserDefinedRerankingModel,
};
use std::path::Path;
use std::sync::Mutex;

/// The model graph, at the path the pinned repository uses.
pub const MODEL_FILE: &str = "onnx/model.onnx";
/// Longest (query, passage) pair the model reads. A cross-encoder reads both texts together, so a
/// long passage is cut at this limit.
const MAX_LENGTH: usize = 512;
const BATCH_SIZE: usize = 16;

/// Files the reranker needs that are absent from `model_dir`, or present with the wrong size.
/// Sizes are checked here because hashing a 151 MB file on every load would cost more than the
/// check is worth. Hashes are checked once, by the installer.
pub fn missing_files(model_dir: &Path) -> Vec<&'static str> {
    install::ARTIFACTS
        .iter()
        .filter(|artifact| !is_present(model_dir, artifact))
        .map(|artifact| artifact.path)
        .collect()
}

fn is_present(model_dir: &Path, artifact: &install::Artifact) -> bool {
    std::fs::metadata(model_dir.join(artifact.path))
        .map(|metadata| metadata.is_file() && metadata.len() == artifact.size)
        .unwrap_or(false)
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

    /// Create every pinned artifact at its pinned size. The contents are zeros, so the files are
    /// the right shape for the presence check but not a loadable model.
    fn write_pinned_sizes(dir: &Path) {
        for artifact in install::ARTIFACTS.iter() {
            let path = dir.join(artifact.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path)
                .unwrap()
                .set_len(artifact.size)
                .unwrap();
        }
    }

    #[test]
    fn missing_files_lists_every_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(missing_files(dir.path()).len(), install::ARTIFACTS.len());
    }

    #[test]
    fn missing_files_is_empty_when_all_files_have_pinned_sizes() {
        let dir = tempfile::tempdir().unwrap();
        write_pinned_sizes(dir.path());
        assert!(missing_files(dir.path()).is_empty());
    }

    #[test]
    fn a_truncated_file_is_reported_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_pinned_sizes(dir.path());
        std::fs::File::create(dir.path().join(MODEL_FILE))
            .unwrap()
            .set_len(10)
            .unwrap();
        assert_eq!(missing_files(dir.path()), vec![MODEL_FILE]);
    }

    #[test]
    fn load_rejects_an_unreadable_model_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        write_pinned_sizes(dir.path());
        let result = OnnxCrossEncoder::load_cached(dir.path(), EmbeddingDevice::Cpu, 1);
        assert!(result.is_err());
    }

    /// Smoke test against the real installed model. It needs `BPX_RERANK_MODEL_DIR` to point at an
    /// installed reranker, so it is ignored by default:
    /// `BPX_RERANK_MODEL_DIR=<dir> cargo test --features gpu-cuda -- --ignored installed_model`
    fn installed_model() -> OnnxCrossEncoder {
        let dir = std::env::var_os("BPX_RERANK_MODEL_DIR")
            .expect("set BPX_RERANK_MODEL_DIR to an installed reranker model");
        OnnxCrossEncoder::load_cached(Path::new(&dir), EmbeddingDevice::Cpu, 4)
            .expect("the installed model loads through the adapter")
    }

    #[test]
    #[ignore]
    fn installed_model_ranks_a_relevant_passage_above_unrelated_ones() {
        let reranker = installed_model();
        let query = "how do I parse a JSON string in Rust";
        let passages = [
            "serde_json::from_str deserializes a JSON string into a Rust value; it returns a Result for parse errors.",
            "The river flows south through the valley, carrying sediment toward the delta at sunset.",
            "Parsing JSON in Rust: use serde_json::from_str with a type that derives Deserialize.",
            "Recipe: whisk two eggs with sugar, fold in flour, and bake for twenty minutes.",
        ];

        let scores = reranker.score(query, &passages).unwrap();
        eprintln!("scores {scores:?}");

        assert_eq!(scores.len(), passages.len());
        assert!(scores.iter().all(|score| score.is_finite()), "{scores:?}");
        assert!(scores[0] > scores[1] && scores[0] > scores[3], "{scores:?}");
        assert!(scores[2] > scores[1] && scores[2] > scores[3], "{scores:?}");
    }

    #[test]
    #[ignore]
    fn installed_model_scores_a_full_candidate_batch_consistently() {
        let reranker = installed_model();
        let query = "how do I parse a JSON string in Rust";
        let passages: Vec<String> = (0..30)
            .map(|index| format!("candidate {index}: notes about parsing configuration files and JSON input {index}"))
            .collect();
        let borrowed: Vec<&str> = passages.iter().map(String::as_str).collect();

        let started = std::time::Instant::now();
        let batch = reranker.score(query, &borrowed).unwrap();
        let elapsed = started.elapsed();

        assert_eq!(batch.len(), passages.len());
        assert!(batch.iter().all(|score| score.is_finite()));
        // A passage scores the same alone as inside a batch of 30, up to floating-point noise.
        let alone = reranker.score(query, &borrowed[17..18]).unwrap();
        assert!(
            (alone[0] - batch[17]).abs() < 1e-3,
            "{} vs {}",
            alone[0],
            batch[17]
        );
        eprintln!("scored 30 candidates in {elapsed:?}");
    }
}
