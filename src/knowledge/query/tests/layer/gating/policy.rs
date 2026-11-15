use super::super::super::super::layers::{LayerOutcome, LayerPolicy};
use super::super::super::super::*;
use super::super::super::{make_store, seed_chunk};
use super::super::{layer, lexical_only, ALL_POLICIES};

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
