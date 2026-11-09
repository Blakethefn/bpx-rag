//! Which layers run for each query shape, and what the diagnostics report.

use super::super::layers::{LayerOutcome, LayerPolicy, LayerReport, SkipReason};
use super::super::*;
use super::{make_store, seed_chunk};
use crate::embed::Embed;
use std::sync::Arc;

struct BrokenQueryEmbedder;

impl Embed for BrokenQueryEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow::anyhow!("synthetic query embedding failure"))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
    }

    fn dim(&self) -> usize {
        4
    }
}

const ALL_POLICIES: [LayerPolicy; 3] = [
    LayerPolicy::CurrentGating,
    LayerPolicy::OwnPoolGating,
    LayerPolicy::Unconditional,
];

fn lexical_only() -> SearchWeights {
    SearchWeights {
        keyword_weight: 1.0,
        vector_weight: 0.0,
    }
}

fn layer<'a>(report: &'a SearchReport, name: &str) -> &'a LayerReport {
    report
        .layers
        .iter()
        .find(|layer| layer.layer == name)
        .unwrap_or_else(|| panic!("no diagnostics for layer {name}"))
}

fn skipped(reason: SkipReason) -> LayerOutcome {
    LayerOutcome::Skipped { reason }
}

/// Seed `count` chunks in source "repo" that all contain `word`.
fn seed_repeated_word(store: &KnowledgeStore, count: usize, word: &str) {
    for index in 0..count {
        seed_chunk(
            store,
            "repo",
            "/repo",
            &format!("notes/{index}.md"),
            &format!("Note {index}"),
            &format!("entry {index} mentions {word} once"),
            "{}",
        );
    }
}

#[test]
fn every_layer_is_reported_in_pipeline_order() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha beta", "{}");

    let report = search_with_pool(
        &store,
        "alpha beta",
        SearchScope {
            filter: None,
            source_label: None,
            policy: LayerPolicy::CurrentGating,
            max_per_file: 1,
        },
        5,
        50,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    let names: Vec<&str> = report.layers.iter().map(|layer| layer.layer).collect();
    assert_eq!(names, ["bm25", "trigram", "fuzzy", "identifier", "vector"]);
}

#[test]
fn short_query_reports_trigram_as_too_short_and_adds_no_warning() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "ab ab ab", "{}");

    let report =
        knowledge_search_report(&store, "ab", None, None, 5, &lexical_only(), 1_000).unwrap();

    assert_eq!(
        layer(&report, "trigram").outcome,
        skipped(SkipReason::QueryTooShort)
    );
    assert_eq!(layer(&report, "bm25").outcome, LayerOutcome::Ran);
    assert!(
        report.warnings.is_empty(),
        "intentional skips are not warnings"
    );
}

#[test]
fn single_content_word_reports_identifier_as_too_few_words() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha", "{}");

    let report =
        knowledge_search_report(&store, "alpha", None, None, 5, &lexical_only(), 1_000).unwrap();

    assert_eq!(
        layer(&report, "identifier").outcome,
        skipped(SkipReason::TooFewContentWords)
    );
}

#[test]
fn exact_hit_reports_bm25_candidates() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha beta", "{}");
    seed_chunk(&store, "repo", "/repo", "b.md", "B", "gamma delta", "{}");

    let report =
        knowledge_search_report(&store, "alpha beta", None, None, 5, &lexical_only(), 1_000)
            .unwrap();

    let bm25 = layer(&report, "bm25");
    assert_eq!(bm25.outcome, LayerOutcome::Ran);
    assert_eq!(bm25.candidates, 1);
}

#[test]
fn no_hit_query_reports_no_failed_layer_and_no_warning() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha beta", "{}");

    let report = knowledge_search_report(
        &store,
        "zebra quartz",
        None,
        None,
        5,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    assert!(report.results.is_empty());
    assert!(report.warnings.is_empty());
    assert!(report
        .layers
        .iter()
        .all(|layer| !matches!(layer.outcome, LayerOutcome::Failed { .. })));
    assert_eq!(layer(&report, "bm25").candidates, 0);
}

#[test]
fn full_pool_skips_trigram_under_current_gating_with_pool_reason() {
    let store = make_store();
    seed_repeated_word(&store, 5, "alpha");

    let report = search_with_pool(
        &store,
        "alpha",
        SearchScope {
            filter: None,
            source_label: None,
            policy: LayerPolicy::CurrentGating,
            max_per_file: 1,
        },
        2,
        3,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    assert_eq!(layer(&report, "bm25").candidates, 3);
    assert_eq!(
        layer(&report, "trigram").outcome,
        skipped(SkipReason::PoolFull)
    );
    assert_eq!(
        layer(&report, "fuzzy").outcome,
        skipped(SkipReason::PoolFull)
    );
}

#[test]
fn full_pool_still_runs_trigram_under_unconditional_policy() {
    let store = make_store();
    seed_repeated_word(&store, 5, "alpha");

    let report = search_with_pool(
        &store,
        "alpha",
        SearchScope {
            filter: None,
            source_label: None,
            policy: LayerPolicy::Unconditional,
            max_per_file: 1,
        },
        2,
        3,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    assert_eq!(layer(&report, "trigram").outcome, LayerOutcome::Ran);
    assert!(layer(&report, "trigram").candidates > 0);
    // "alpha" is already an indexed word, so there is nothing to correct.
    assert_eq!(
        layer(&report, "fuzzy").outcome,
        skipped(SkipReason::NoCorrectedWord)
    );
}

#[test]
fn own_pool_policy_runs_fuzzy_when_only_trigram_filled_the_pool() {
    let store = make_store();
    // "alp" is not an indexed word, so BM25 finds nothing. Trigram finds three chunks,
    // which fill the combined pool of three.
    for index in 0..3 {
        seed_chunk(
            &store,
            "repo",
            "/repo",
            &format!("words/{index}.md"),
            &format!("Word {index}"),
            &format!("alphabet entry {index}"),
            "{}",
        );
    }

    let current = search_with_pool(
        &store,
        "alp",
        SearchScope {
            filter: None,
            source_label: None,
            policy: LayerPolicy::CurrentGating,
            max_per_file: 1,
        },
        2,
        3,
        &lexical_only(),
        1_000,
    )
    .unwrap();
    let own_pool = search_with_pool(
        &store,
        "alp",
        SearchScope {
            filter: None,
            source_label: None,
            policy: LayerPolicy::OwnPoolGating,
            max_per_file: 1,
        },
        2,
        3,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    assert_eq!(layer(&current, "trigram").candidates, 3);
    assert_eq!(
        layer(&current, "fuzzy").outcome,
        skipped(SkipReason::PoolFull)
    );
    assert_eq!(
        layer(&own_pool, "fuzzy").outcome,
        skipped(SkipReason::NoCorrectedWord)
    );
}

#[test]
fn metadata_filter_applies_to_every_layer_that_runs() {
    let store = make_store();
    seed_chunk(&store, "keep", "/keep", "k.md", "K", "alpha notes", "{}");
    seed_chunk(&store, "drop", "/drop", "d.md", "D", "alpha notes", "{}");

    let report = search_with_pool(
        &store,
        "alpha notes",
        SearchScope {
            filter: None,
            source_label: Some("keep"),
            policy: LayerPolicy::Unconditional,
            max_per_file: 1,
        },
        5,
        50,
        &lexical_only(),
        1_000,
    )
    .unwrap();

    assert_eq!(layer(&report, "trigram").outcome, LayerOutcome::Ran);
    assert_eq!(layer(&report, "trigram").candidates, 1);
    assert!(report
        .results
        .iter()
        .all(|result| result.source_label == "keep"));
}

#[test]
fn policies_return_identical_results_when_no_gate_fires() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "a.md",
        "A",
        "alpha beta gamma",
        "{}",
    );
    seed_chunk(&store, "repo", "/repo", "b.md", "B", "beta delta", "{}");
    seed_chunk(&store, "repo", "/repo", "c.md", "C", "gamma epsilon", "{}");

    for query in ["alpha beta", "gamma", "beta delta epsilon"] {
        let runs: Vec<SearchReport> = ALL_POLICIES
            .iter()
            .map(|policy| {
                search_with_pool(
                    &store,
                    query,
                    SearchScope {
                        filter: None,
                        source_label: None,
                        policy: *policy,
                        max_per_file: 1,
                    },
                    5,
                    50,
                    &lexical_only(),
                    1_000,
                )
                .unwrap()
            })
            .collect();
        for run in &runs[1..] {
            let left: Vec<(&str, f64)> = runs[0]
                .results
                .iter()
                .map(|r| (r.file_path.as_str(), r.score))
                .collect();
            let right: Vec<(&str, f64)> = run
                .results
                .iter()
                .map(|r| (r.file_path.as_str(), r.score))
                .collect();
            assert_eq!(left, right, "query {query:?}");
        }
    }
}

#[test]
fn disabled_lexical_layers_report_disabled_and_missing_embedder_is_reported() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha beta", "{}");
    let weights = SearchWeights {
        keyword_weight: 0.0,
        vector_weight: 1.0,
    };

    let report =
        knowledge_search_report(&store, "alpha beta", None, None, 5, &weights, 1_000).unwrap();

    for name in ["bm25", "trigram", "fuzzy", "identifier"] {
        assert_eq!(
            layer(&report, name).outcome,
            skipped(SkipReason::Disabled),
            "{name}"
        );
    }
    assert_eq!(
        layer(&report, "vector").outcome,
        skipped(SkipReason::NoEmbedder)
    );
}

#[test]
fn failed_query_embedding_is_a_failed_vector_layer_with_a_warning() {
    let mut store = make_store();
    store.set_embedder(Arc::new(BrokenQueryEmbedder));
    seed_chunk(&store, "repo", "/repo", "a.md", "A", "alpha beta", "{}");
    let weights = SearchWeights::default();

    let report =
        knowledge_search_report(&store, "alpha beta", None, None, 5, &weights, 1_000).unwrap();

    match &layer(&report, "vector").outcome {
        LayerOutcome::Failed { error } => {
            assert!(error.contains("synthetic query embedding failure"))
        }
        other => panic!("expected a failed vector layer, got {other:?}"),
    }
    assert_eq!(report.warnings.len(), 1);
}
