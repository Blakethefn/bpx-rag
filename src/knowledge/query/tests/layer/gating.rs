use super::super::super::layers::{LayerOutcome, LayerPolicy, SkipReason};
use super::super::super::*;
use super::super::{make_store, seed_chunk};
use super::{ALL_POLICIES, lexical_only, layer, seed_repeated_word, skipped};

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
