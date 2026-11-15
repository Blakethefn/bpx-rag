use super::super::super::layers::{LayerOutcome, LayerPolicy, SkipReason};
use super::super::super::*;
use super::super::{make_store, seed_chunk};
use super::{layer, lexical_only, seed_repeated_word, skipped};

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
    assert_eq!(
        layer(&report, "fuzzy").outcome,
        skipped(SkipReason::NoCorrectedWord)
    );
}

#[test]
fn own_pool_policy_runs_fuzzy_when_only_trigram_filled_the_pool() {
    let store = make_store();
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

mod policy;
