use super::super::super::*;
use super::{chunk_rowids, symbol_names};
use crate::knowledge::open_in_memory;
use crate::support::add_synced_source;
use std::fs;
use tempfile::tempdir;

fn source_of(store: &KnowledgeStore) -> KnowledgeSourceInfo {
    store.get_source("test").unwrap().unwrap()
}

#[test]
fn reindex_matches_a_full_sync_for_the_same_changes() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    let source = add_synced_source(&store, "test", dir.path());

    fs::write(
        dir.path().join("a.rs"),
        "pub fn alpha() -> u8 { 1 }\npub fn gamma() -> u8 { 3 }\n",
    )
    .unwrap();
    sync_paths(&store, &source, None, &["a.rs".to_string()]).unwrap();
    let partial_chunks = chunk_rowids(&store, "a.rs").len();

    let full = sync_source(&store, &source_of(&store), None).unwrap();
    assert_eq!(
        full.files_updated, 0,
        "a path reindex must leave nothing for a full sync"
    );
    assert_eq!(chunk_rowids(&store, "a.rs").len(), partial_chunks);
    assert_eq!(symbol_names(&store), vec!["alpha", "gamma"]);
}
