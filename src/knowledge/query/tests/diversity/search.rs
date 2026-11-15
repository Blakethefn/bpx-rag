use super::super::super::layers::LayerPolicy;
use super::super::super::*;
use super::super::{make_store, seed_chunk};

const WEIGHTS: SearchWeights = SearchWeights {
    keyword_weight: 1.0,
    vector_weight: 0.0,
};

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

    assert_eq!(
        placements(&report),
        pairs(&[("only.md", "A1"), ("only.md", "A2"), ("only.md", "A3")]),
    );
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

mod limit;
