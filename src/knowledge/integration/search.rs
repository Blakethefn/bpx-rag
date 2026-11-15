use super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use std::path::Path;
use tempfile::tempdir;

mod filter;

#[test]
pub(super) fn empty_knowledge_search_returns_empty() {
    let store = make_store();
    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "anything at all", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty());
}

#[test]
pub(super) fn empty_knowledge_filtered_search_returns_empty() {
    let store = make_store();
    let weights = SearchWeights::default();
    let results = query::knowledge_search(
        &store,
        "anything",
        Some("type:task status:active"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(results.is_empty());
}

#[test]
pub(super) fn multi_source_with_source_filter() {
    let store = make_store();
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();

    std::fs::write(
        dir1.path().join("v.md"),
        "# Vault Note\n\nContent about the holographic rendering pipeline.",
    )
    .unwrap();
    std::fs::write(
        dir2.path().join("d.md"),
        "# Docs Note\n\nContent about the holographic rendering pipeline.",
    )
    .unwrap();

    store
        .add_source("vault", dir1.path().to_str().unwrap(), &[], &[])
        .unwrap();
    store
        .add_source("docs", dir2.path().to_str().unwrap(), &[], &[])
        .unwrap();

    let s1 = store.get_source("vault").unwrap().unwrap();
    let s2 = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &s1, None).unwrap();
    sync::sync_source(&store, &s2, None).unwrap();

    let weights = SearchWeights::default();

    let results = query::knowledge_search(
        &store,
        "holographic rendering",
        None,
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(
        results.len(),
        2,
        "unfiltered should return from both sources"
    );

    let results = query::knowledge_search(
        &store,
        "holographic rendering",
        Some("source:vault"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "vault");

    let results = query::knowledge_search(
        &store,
        "holographic rendering",
        None,
        Some("docs"),
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "docs");
}
