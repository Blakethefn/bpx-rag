//! Reranking of the fused candidate list: placement, diagnostics, and fallbacks.

use super::layers::{LayerOutcome, LayerReport, SkipReason};
use super::tests::{make_store, seed_chunk};
use super::*;
use crate::rerank::{Rerank, RerankSlot};
use crate::search::profile::SearchWhy;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Scores passages that contain `preferred` at 1.0 and the rest at 0.0. Records how many
/// passages it was asked to score.
struct PreferText {
    preferred: &'static str,
    passages_seen: AtomicUsize,
}

impl PreferText {
    fn new(preferred: &'static str) -> Arc<Self> {
        Arc::new(Self {
            preferred,
            passages_seen: AtomicUsize::new(0),
        })
    }
}

impl Rerank for PreferText {
    fn score(&self, _query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        self.passages_seen.store(passages.len(), Ordering::SeqCst);
        Ok(passages
            .iter()
            .map(|passage| {
                if passage.contains(self.preferred) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect())
    }
}

struct FailingReranker;

impl Rerank for FailingReranker {
    fn score(&self, _query: &str, _passages: &[&str]) -> Result<Vec<f32>> {
        Err(anyhow::anyhow!("synthetic reranker failure"))
    }
}

struct NonFiniteReranker;

impl Rerank for NonFiniteReranker {
    fn score(&self, _query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        Ok(vec![f32::NAN; passages.len()])
    }
}

fn lexical_only() -> SearchWeights {
    SearchWeights {
        keyword_weight: 1.0,
        vector_weight: 0.0,
    }
}

fn scope() -> SearchScope<'static> {
    SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::CurrentGating,
    }
}

/// Three files that all match "alpha".
fn seed_three_files(store: &KnowledgeStore) {
    seed_chunk(
        store,
        "repo",
        "/repo",
        "a.md",
        "A",
        "alpha first note",
        "{}",
    );
    seed_chunk(
        store,
        "repo",
        "/repo",
        "b.md",
        "B",
        "alpha second note",
        "{}",
    );
    seed_chunk(
        store,
        "repo",
        "/repo",
        "c.md",
        "C",
        "alpha third note",
        "{}",
    );
}

fn run(store: &KnowledgeStore, limit: u32) -> SearchReport {
    search_with_pool(store, "alpha", scope(), limit, 50, &lexical_only(), 1024).unwrap()
}

fn rerank_layer(report: &SearchReport) -> Option<&LayerReport> {
    report.layers.iter().find(|layer| layer.layer == "rerank")
}

fn paths(report: &SearchReport) -> Vec<(String, f64)> {
    report
        .results
        .iter()
        .map(|result| (result.file_path.clone(), result.score))
        .collect()
}

#[test]
fn unconfigured_store_reports_no_rerank_layer() {
    let store = make_store();
    seed_three_files(&store);

    let report = run(&store, 5);

    assert!(rerank_layer(&report).is_none());
    assert!(report.warnings.is_empty());
}

#[test]
fn reranker_reorders_the_head_before_file_diversification() {
    let mut store = make_store();
    seed_three_files(&store);
    let reranker = PreferText::new("third note");
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Ok(reranker.clone()),
    });

    let report = run(&store, 5);

    assert_eq!(report.results[0].file_path, "c.md");
    assert_eq!(report.results[0].why.rerank_score, Some(1.0));
    assert!(report
        .results
        .windows(2)
        .all(|pair| pair[0].score >= pair[1].score));
    let layer = rerank_layer(&report).expect("the rerank layer is reported");
    assert_eq!(layer.outcome, LayerOutcome::Ran);
    assert_eq!(layer.candidates, 3);
}

#[test]
fn candidate_limit_is_respected() {
    let mut store = make_store();
    seed_three_files(&store);
    let reranker = PreferText::new("third note");
    store.set_reranker(RerankSlot {
        candidates: 2,
        reranker: Ok(reranker.clone()),
    });

    let report = run(&store, 5);

    assert_eq!(reranker.passages_seen.load(Ordering::SeqCst), 2);
    assert_eq!(rerank_layer(&report).unwrap().candidates, 2);
    let scored = report
        .results
        .iter()
        .filter(|result| result.why.rerank_score.is_some())
        .count();
    assert_eq!(scored, 2);
}

#[test]
fn missing_model_keeps_the_fused_order_and_reports_the_failure() {
    let mut store = make_store();
    seed_three_files(&store);
    let baseline = run(&store, 5);
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Err("reranker model files are missing in /models: model.onnx".to_string()),
    });

    let report = run(&store, 5);

    assert_eq!(paths(&report), paths(&baseline));
    assert!(report
        .results
        .iter()
        .all(|result| result.why.rerank_score.is_none()));
    match &rerank_layer(&report)
        .expect("the rerank layer is reported")
        .outcome
    {
        LayerOutcome::Failed { error } => assert!(error.contains("model.onnx"), "{error}"),
        other => panic!("expected a failed rerank layer, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("model.onnx")));
}

#[test]
fn reranker_error_keeps_the_fused_order() {
    let mut store = make_store();
    seed_three_files(&store);
    let baseline = run(&store, 5);
    let failing: Arc<dyn Rerank> = Arc::new(FailingReranker);
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Ok(failing),
    });

    let report = run(&store, 5);

    assert_eq!(paths(&report), paths(&baseline));
    match &rerank_layer(&report)
        .expect("the rerank layer is reported")
        .outcome
    {
        LayerOutcome::Failed { error } => {
            assert!(error.contains("synthetic reranker failure"), "{error}")
        }
        other => panic!("expected a failed rerank layer, got {other:?}"),
    }
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("synthetic reranker failure")));
}

#[test]
fn non_finite_scores_keep_the_fused_order() {
    let mut store = make_store();
    seed_three_files(&store);
    let baseline = run(&store, 5);
    let broken: Arc<dyn Rerank> = Arc::new(NonFiniteReranker);
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Ok(broken),
    });

    let report = run(&store, 5);

    assert_eq!(paths(&report), paths(&baseline));
    assert!(matches!(
        rerank_layer(&report).unwrap().outcome,
        LayerOutcome::Failed { .. }
    ));
}

#[test]
fn a_single_candidate_is_a_skip_not_a_failure() {
    let mut store = make_store();
    seed_three_files(&store);
    let baseline = run(&store, 5);
    store.set_reranker(RerankSlot {
        candidates: 1,
        reranker: Ok(PreferText::new("third note")),
    });

    let report = run(&store, 5);

    assert_eq!(paths(&report), paths(&baseline));
    assert_eq!(
        rerank_layer(&report).unwrap().outcome,
        LayerOutcome::Skipped {
            reason: SkipReason::TooFewCandidates
        }
    );
    assert!(report.warnings.is_empty());
}

#[test]
fn constant_scores_keep_the_fused_order() {
    let mut store = make_store();
    seed_three_files(&store);
    let baseline = run(&store, 5);
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Ok(PreferText::new("no passage contains this")),
    });

    let report = run(&store, 5);

    assert_eq!(paths(&report), paths(&baseline));
}

#[test]
fn reranked_order_is_deterministic_across_repeated_searches() {
    let mut store = make_store();
    seed_three_files(&store);
    store.set_reranker(RerankSlot {
        candidates: 3,
        reranker: Ok(PreferText::new("second note")),
    });

    assert_eq!(paths(&run(&store, 5)), paths(&run(&store, 5)));
}

#[test]
fn explain_line_shows_the_rerank_score() {
    let why = SearchWhy {
        rerank_score: Some(0.25),
        ..Default::default()
    };

    assert!(why.detailed_line(0.5).contains("rerank=0.2500"));
}
