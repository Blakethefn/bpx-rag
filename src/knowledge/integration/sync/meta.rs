use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use std::path::Path;
use tempfile::tempdir;

// ── Test Anchor 4: Empty knowledge graceful handling ───────────────────
// "With no sources registered, search returns empty, not error."

#[test]
fn sync_with_enrichments_updates_metadata() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();

    let file_path = dir.path().join("evolving.md");
    std::fs::write(
        &file_path,
        "---\ntype: task\nstatus: active\n---\n# Task\n\nWorking on the quantum simulator.",
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

    // Verify initial state: searchable with type:task
    let weights = SearchWeights::default();
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);

    // Change the type from task → spec
    std::fs::write(
        &file_path,
        "---\ntype: spec\nstatus: done\n---\n# Spec\n\nCompleted quantum simulator specification.",
    )
    .unwrap();

    let source = store.get_source("vault").unwrap().unwrap();
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    // Old filter should no longer match
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(
        results.is_empty(),
        "type:task should no longer match after status change"
    );

    // New filter should match
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:spec"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1, "type:spec should match after sync");
    assert_eq!(results[0].metadata["frontmatter"]["status"], "done");
}
