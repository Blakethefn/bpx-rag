use super::super::super::super::layers::LayerPolicy;
use super::super::super::super::*;
use super::{pairs, placements, seed_corpus, WEIGHTS};

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
