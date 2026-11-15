use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use rusqlite::params;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn sync_after_edit_updates_content() {
    let store = super::super::super::make_store();
    let dir = tempdir().unwrap();

    let file_path = dir.path().join("evolving.md");
    std::fs::write(
        &file_path,
        "# Alpha Version\n\nOriginal implementation of the quasar protocol.",
    )
    .unwrap();

    store
        .add_source("docs", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty(), "original content should be searchable");

    std::fs::write(
        &file_path,
        "# Beta Version\n\nReplacement nebula framework with improved throughput.",
    )
    .unwrap();

    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let results =
        query::knowledge_search(&store, "nebula framework", None, None, 10, &weights, 2000)
            .unwrap();
    assert!(
        !results.is_empty(),
        "updated content should be searchable after sync"
    );

    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty(), "old content should be gone after sync");
}

#[test]
fn sync_nonexistent_source_path_errors_gracefully() {
    let store = super::super::super::make_store();
    let dir = tempdir().unwrap();
    let path_str = dir.path().to_str().unwrap().to_string();

    store.add_source("temp", &path_str, &[], &[]).unwrap();
    let source = store.get_source("temp").unwrap().unwrap();

    drop(dir);

    let result = sync::sync_source(&store, &source, None);
    assert!(
        result.is_err(),
        "sync should error when source path is gone"
    );
}
