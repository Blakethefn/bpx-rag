//! Optional cross-encoder reranking of fused search candidates.
//!
//! Off by default. When it is enabled, the top fused candidates of a search are reordered by
//! reranker score before file diversification. A reranker that is missing or fails leaves the
//! fused order in place and is reported in the layer diagnostics and as a warning. A search never
//! fails because of the reranker.

pub mod install;
pub mod onnx;

use crate::config::{Config, RerankConfig};
use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::Arc;

/// Scores (query, passage) pairs. Higher scores mean more relevant.
pub trait Rerank: Send + Sync {
    /// One score per passage, in input order.
    fn score(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>>;
}

/// The reranker a store uses. A configured reranker that could not be loaded is kept as an error,
/// so each search can say why it ran without reranking.
#[derive(Clone)]
pub struct RerankSlot {
    /// Fused candidates to reorder, from the top of the list.
    pub candidates: usize,
    pub reranker: Result<Arc<dyn Rerank>, String>,
}

/// The slot for a config. `None` when reranking is off, so the default search path is unchanged.
pub fn slot_from_config(config: &Config) -> Option<RerankSlot> {
    let rerank: &RerankConfig = &config.search.rerank;
    if !rerank.enabled {
        return None;
    }
    let reranker = onnx::OnnxCrossEncoder::load_cached(
        Path::new(&rerank.model_dir),
        rerank.device,
        config.embeddings.intra_threads,
    )
    .map(|reranker| Arc::new(reranker) as Arc<dyn Rerank>)
    .map_err(|error| format!("{error:#}"));
    Some(RerankSlot {
        candidates: rerank.candidates,
        reranker,
    })
}

/// Input indexes with their scores, highest first. Ties keep input order, so the result is
/// deterministic. Rejects a score count that differs from the passage count, and any score that is
/// not finite.
pub fn order_by_score(scores: &[f32], passage_count: usize) -> Result<Vec<(usize, f32)>> {
    if scores.len() != passage_count {
        return Err(anyhow!(
            "reranker returned {} scores for {passage_count} passages",
            scores.len()
        ));
    }
    if let Some(index) = scores.iter().position(|score| !score.is_finite()) {
        return Err(anyhow!(
            "reranker returned a non-finite score for passage {index}"
        ));
    }
    let mut ordered: Vec<(usize, f32)> = scores.iter().copied().enumerate().collect();
    ordered.sort_by(|(left_index, left), (right_index, right)| {
        right.total_cmp(left).then(left_index.cmp(right_index))
    });
    Ok(ordered)
}

/// One line for `models status`: whether reranking is on, and whether its model files are present.
pub fn status_line(config: &Config) -> String {
    let rerank = &config.search.rerank;
    if !rerank.enabled {
        return "reranker: disabled (set search.rerank.enabled = true to use it)".to_string();
    }
    let model_dir = Path::new(&rerank.model_dir);
    let missing = onnx::missing_files(model_dir);
    if missing.is_empty() {
        format!(
            "reranker: enabled, {} candidates, model files present in {}",
            rerank.candidates,
            model_dir.display()
        )
    } else {
        format!(
            "reranker: enabled, but model files are missing or damaged in {}: {}. Run `bpx-rag models download-reranker`.",
            model_dir.display(),
            missing.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn order_by_score_sorts_descending_and_breaks_ties_by_input_order() {
        let order = order_by_score(&[0.5, 0.9, 0.5, 0.1], 4).unwrap();
        assert_eq!(order, vec![(1, 0.9), (0, 0.5), (2, 0.5), (3, 0.1)]);
    }

    #[test]
    fn order_by_score_is_deterministic_for_repeated_calls() {
        let scores = [0.3, 0.3, 0.7];
        assert_eq!(
            order_by_score(&scores, 3).unwrap(),
            order_by_score(&scores, 3).unwrap()
        );
    }

    #[test]
    fn order_by_score_rejects_non_finite_scores() {
        assert!(order_by_score(&[0.1, f32::NAN], 2).is_err());
        assert!(order_by_score(&[f32::INFINITY, 0.1], 2).is_err());
    }

    #[test]
    fn order_by_score_rejects_a_score_count_that_does_not_match() {
        assert!(order_by_score(&[0.1], 2).is_err());
    }

    #[test]
    fn reranking_is_off_by_default() {
        assert!(!Config::default().search.rerank.enabled);
        assert!(slot_from_config(&Config::default()).is_none());
    }

    #[test]
    fn rerank_config_parses_enable_flag_and_candidate_count() {
        let config: RerankConfig = toml::from_str("enabled = true\ncandidates = 12\n").unwrap();
        assert!(config.enabled);
        assert_eq!(config.candidates, 12);
        assert_eq!(RerankConfig::default().candidates, 30);
    }

    #[test]
    fn enabled_config_with_missing_model_yields_an_error_slot() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.search.rerank.enabled = true;
        config.search.rerank.model_dir = dir.path().display().to_string();
        let slot = slot_from_config(&config).expect("an enabled config yields a slot");
        assert_eq!(slot.candidates, 30);
        let reason = slot
            .reranker
            .err()
            .expect("a missing model is reported, not loaded");
        assert!(reason.contains("model.onnx"), "{reason}");
    }

    #[test]
    fn status_line_reports_disabled_and_missing_files() {
        let mut config = Config::default();
        assert!(status_line(&config).starts_with("reranker: disabled"));
        let dir = tempfile::tempdir().unwrap();
        config.search.rerank.enabled = true;
        config.search.rerank.model_dir = dir.path().display().to_string();
        assert!(status_line(&config).contains("missing"));
    }
}
