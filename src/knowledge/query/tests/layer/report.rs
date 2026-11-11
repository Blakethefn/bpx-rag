use super::super::super::layers::{LayerOutcome, LayerPolicy, SkipReason};
use super::super::super::*;
use super::super::{make_store, seed_chunk};
use super::{BrokenQueryEmbedder, lexical_only, layer, skipped};
use std::sync::Arc;

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
