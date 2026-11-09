//! Per-file diversification. The default must keep the pre-Stage-9 output exactly.

use super::super::layers::LayerPolicy;
use super::super::*;
use super::{make_store, seed_chunk};
use crate::search::profile::SearchWhy;

const WEIGHTS: SearchWeights = SearchWeights {
    keyword_weight: 1.0,
    vector_weight: 0.0,
};

/// File `a.md` has four matching chunks, `b.md` has two, and `c.md` has one.
/// The repeat counts of "retry" give a fixed BM25 order that does not depend on row order.
fn seed_corpus() -> KnowledgeStore {
    let store = make_store();
    let chunks = [
        ("a.md", "A1", "retry retry retry retry retry retry"),
        ("a.md", "A2", "retry retry retry retry"),
        ("b.md", "B1", "retry retry retry"),
        ("a.md", "A3", "retry retry"),
        ("c.md", "C1", "retry retry retry retry retry"),
        ("b.md", "B2", "retry"),
        ("a.md", "A4", "retry"),
    ];
    for (path, title, content) in chunks {
        seed_chunk(&store, "repo", "/repo", path, title, content, "{}");
    }
    store
}

fn placements(report: &SearchReport) -> Vec<(String, String)> {
    report
        .results
        .iter()
        .map(|result| (result.file_path.clone(), result.title.clone()))
        .collect()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(path, title)| (path.to_string(), title.to_string()))
        .collect()
}

#[test]
fn default_output_matches_recorded_pre_stage_9_order() {
    let store = seed_corpus();
    let report = knowledge_search_report(&store, "retry", None, None, 4, &WEIGHTS, 1_000).unwrap();

    // Recorded from the HEAD 1e85bda implementation before Stage 9 changed any code.
    // One passage per file comes first in score order (A1, C1, B1). The remaining slot
    // is filled with the best overflow passage, A2.
    assert_eq!(
        placements(&report),
        pairs(&[
            ("a.md", "A1"),
            ("c.md", "C1"),
            ("b.md", "B1"),
            ("a.md", "A2")
        ]),
    );
}

#[test]
fn default_output_fills_from_one_file_when_it_is_the_only_file() {
    let store = make_store();
    for (title, content) in [
        ("A1", "retry retry retry"),
        ("A2", "retry retry"),
        ("A3", "retry"),
    ] {
        seed_chunk(&store, "repo", "/repo", "only.md", title, content, "{}");
    }
    let report = knowledge_search_report(&store, "retry", None, None, 3, &WEIGHTS, 1_000).unwrap();

    // Recorded from HEAD 1e85bda: a one-file corpus still returns the requested count.
    assert_eq!(
        placements(&report),
        pairs(&[("only.md", "A1"), ("only.md", "A2"), ("only.md", "A3")]),
    );
}

fn passage(file_id: i64, title: &str) -> KnowledgeSearchResult {
    KnowledgeSearchResult {
        title: title.to_string(),
        snippet: String::new(),
        score: 0.0,
        source_label: "repo".to_string(),
        file_path: format!("file-{file_id}.md"),
        file_id,
        source_id: 1,
        lines: "1-1".to_string(),
        metadata: serde_json::json!({}),
        source_type: "knowledge".to_string(),
        why: SearchWhy::default(),
    }
}

fn titles(results: &[KnowledgeSearchResult]) -> Vec<&str> {
    results.iter().map(|result| result.title.as_str()).collect()
}

#[test]
fn cap_one_takes_one_passage_per_file_then_overflow_in_score_order() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(2, "r2"),
        passage(3, "r3"),
        passage(1, "r4"),
        passage(2, "r5"),
    ];

    let output = rank::diversify_by_file(input, 4, 1);

    assert_eq!(titles(&output), ["r0", "r2", "r3", "r1"]);
}

#[test]
fn cap_two_keeps_two_passages_per_file_and_respects_limit() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(2, "r2"),
        passage(1, "r3"),
        passage(2, "r4"),
        passage(3, "r5"),
    ];

    let output = rank::diversify_by_file(input, 4, 2);

    assert_eq!(titles(&output), ["r0", "r1", "r2", "r4"]);
}

#[test]
fn cap_two_fills_from_overflow_when_files_are_scarce() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(1, "r2"),
        passage(2, "r3"),
    ];

    let output = rank::diversify_by_file(input, 4, 2);

    assert_eq!(titles(&output), ["r0", "r1", "r3", "r2"]);
}

#[test]
fn single_file_corpus_returns_up_to_cap_then_fills_to_limit() {
    let input = (0..6)
        .map(|index| passage(1, &format!("r{index}")))
        .collect();

    let output = rank::diversify_by_file(input, 5, 3);

    assert_eq!(titles(&output), ["r0", "r1", "r2", "r3", "r4"]);
}

#[test]
fn cap_above_limit_behaves_as_score_order_truncated_to_limit() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(1, "r2"),
        passage(2, "r3"),
    ];

    let output = rank::diversify_by_file(input, 3, 10);

    assert_eq!(titles(&output), ["r0", "r1", "r2"]);
}

#[test]
fn fewer_passages_than_limit_returns_all_of_them() {
    let input = vec![passage(1, "r0"), passage(2, "r1")];

    let output = rank::diversify_by_file(input, 5, 1);

    assert_eq!(titles(&output), ["r0", "r1"]);
}

#[test]
fn empty_input_and_zero_limit_return_empty() {
    assert!(rank::diversify_by_file(Vec::new(), 5, 1).is_empty());
    assert!(rank::diversify_by_file(vec![passage(1, "r0")], 0, 1).is_empty());
}

#[test]
fn zero_max_per_file_is_rejected_before_search() {
    let store = seed_corpus();
    let scope = SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::default(),
        max_per_file: 0,
    };

    let error = knowledge_search_report_with_policy(&store, "retry", scope, 4, &WEIGHTS, 1_000)
        .expect_err("max_per_file of 0 must be rejected");

    assert!(error.to_string().contains("max_per_file"), "{error}");
}

#[test]
fn explicit_max_per_file_one_matches_default_search_exactly() {
    let store = seed_corpus();
    let default = knowledge_search_report(&store, "retry", None, None, 4, &WEIGHTS, 1_000).unwrap();
    let scope = SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::default(),
        max_per_file: 1,
    };

    let explicit =
        knowledge_search_report_with_policy(&store, "retry", scope, 4, &WEIGHTS, 1_000).unwrap();

    assert_eq!(placements(&explicit), placements(&default));
}

#[test]
fn max_per_file_two_takes_a_second_passage_from_the_top_file() {
    let store = seed_corpus();
    let scope = SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::default(),
        max_per_file: 2,
    };

    let report =
        knowledge_search_report_with_policy(&store, "retry", scope, 4, &WEIGHTS, 1_000).unwrap();

    // Score order is A1, C1, A2, B1, A3, B2, A4. A2 is kept as the second passage from a.md.
    assert_eq!(
        placements(&report),
        pairs(&[
            ("a.md", "A1"),
            ("c.md", "C1"),
            ("a.md", "A2"),
            ("b.md", "B1")
        ]),
    );
}

#[test]
fn max_per_file_three_keeps_three_passages_from_one_file_before_filling() {
    let store = seed_corpus();
    let scope = SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::default(),
        max_per_file: 3,
    };

    let report =
        knowledge_search_report_with_policy(&store, "retry", scope, 6, &WEIGHTS, 1_000).unwrap();

    assert_eq!(
        placements(&report),
        pairs(&[
            ("a.md", "A1"),
            ("c.md", "C1"),
            ("a.md", "A2"),
            ("b.md", "B1"),
            ("a.md", "A3"),
            ("b.md", "B2"),
        ]),
    );
}

#[test]
fn max_per_file_accepts_the_boundaries_one_and_fifty_and_rejects_fifty_one() {
    let store = seed_corpus();
    let scope_with = |max_per_file| SearchScope {
        filter: None,
        source_label: None,
        policy: LayerPolicy::default(),
        max_per_file,
    };

    for accepted in [1, MAX_PER_FILE] {
        assert!(
            knowledge_search_report_with_policy(
                &store,
                "retry",
                scope_with(accepted),
                4,
                &WEIGHTS,
                1_000
            )
            .is_ok(),
            "max_per_file {accepted} must be accepted"
        );
    }
    let error = knowledge_search_report_with_policy(
        &store,
        "retry",
        scope_with(MAX_PER_FILE + 1),
        4,
        &WEIGHTS,
        1_000,
    )
    .expect_err("max_per_file above the maximum must be rejected");
    assert!(error.to_string().contains("between 1 and 50"), "{error}");
}
