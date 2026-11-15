use super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use std::path::Path;
use tempfile::tempdir;

mod catalog;

#[test]
pub(super) fn enrichment_to_filtered_search_pipeline() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("task.md"),
        "---\ntype: task\nstatus: active\ntags:\n  - implementation\n  - system3\n---\n# Active Task\n\nThis task involves building the prediction engine.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("note.md"),
        "---\ntype: note\nstatus: draft\n---\n# Draft Note\n\nSome draft notes about the prediction engine.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("plain.md"),
        "# Plain File\n\nNo frontmatter here, just content about the prediction engine.",
    )
    .unwrap();

    let enrichments = vec!["frontmatter".to_string()];
    store
        .add_source("vault", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("vault").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    let weights = SearchWeights::default();

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1, "type:task should match exactly one file");
    assert_eq!(results[0].metadata["frontmatter"]["type"], "task");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("type:project"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(results.is_empty(), "type:project should match nothing");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("tag:implementation"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(
        results.len(),
        1,
        "tag:implementation should match exactly one file"
    );
    assert_eq!(results[0].metadata["frontmatter"]["type"], "task");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("status:active"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].metadata["frontmatter"]["status"], "active");

    let results =
        query::knowledge_search(&store, "prediction engine", None, None, 10, &weights, 2000)
            .unwrap();
    assert_eq!(
        results.len(),
        3,
        "unfiltered search should find all three files"
    );
}
