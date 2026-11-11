use super::*;

#[test]
fn current_policy_skips_trigram_only_when_bm25_fills_the_pool() {
    assert_eq!(
        trigram_skip(LayerPolicy::CurrentGating, "alpha", 49, 50),
        None
    );
    assert_eq!(
        trigram_skip(LayerPolicy::CurrentGating, "alpha", 50, 50),
        Some(SkipReason::PoolFull)
    );
}

#[test]
fn current_policy_fuzzy_gate_counts_trigram_additions() {
    // BM25 is below the limit, but BM25 plus trigram has filled it.
    assert_eq!(
        fuzzy_skip(LayerPolicy::CurrentGating, 20, 50, 50),
        Some(SkipReason::PoolFull)
    );
}

#[test]
fn own_pool_policy_fuzzy_gate_ignores_trigram_additions() {
    assert_eq!(fuzzy_skip(LayerPolicy::OwnPoolGating, 20, 50, 50), None);
    assert_eq!(
        fuzzy_skip(LayerPolicy::OwnPoolGating, 50, 50, 50),
        Some(SkipReason::PoolFull)
    );
}

#[test]
fn unconditional_policy_never_gates_on_pool_size() {
    assert_eq!(
        trigram_skip(LayerPolicy::Unconditional, "alpha", 500, 50),
        None
    );
    assert_eq!(fuzzy_skip(LayerPolicy::Unconditional, 500, 500, 50), None);
}

#[test]
fn short_query_skips_trigram_under_every_policy() {
    for policy in [
        LayerPolicy::CurrentGating,
        LayerPolicy::OwnPoolGating,
        LayerPolicy::Unconditional,
    ] {
        assert_eq!(
            trigram_skip(policy, "ab", 0, 50),
            Some(SkipReason::QueryTooShort)
        );
    }
}

#[test]
fn outcome_serializes_with_its_state_and_reason() {
    let skipped = LayerOutcome::Skipped {
        reason: SkipReason::PoolFull,
    };
    assert_eq!(
        serde_json::to_value(&skipped).unwrap(),
        serde_json::json!({"state": "skipped", "reason": "pool_full"})
    );
    let failed = LayerOutcome::Failed {
        error: "boom".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&failed).unwrap(),
        serde_json::json!({"state": "failed", "error": "boom"})
    );
    assert_eq!(
        serde_json::to_value(LayerOutcome::Ran).unwrap(),
        serde_json::json!({"state": "ran"})
    );
}
