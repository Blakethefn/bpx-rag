use super::super::super::*;
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;
use super::{populated_store, table_count};

#[test]
fn removal_after_failed_attempts_removes_everything() {
    let (store, _dir) = populated_store();
    let chunks = table_count(&store, "knowledge_chunk_meta");
    REMOVE_FAILPOINT.with(|point| point.set(Some(1)));
    assert!(store.remove_source("gone").is_err());
    REMOVE_FAILPOINT.with(|point| point.set(None));

    let result = store.remove_source("gone").unwrap();

    assert_eq!(result.files_removed, 2);
    assert_eq!(result.chunks_removed, chunks);
    for table in [
        "knowledge_sources",
        "knowledge_files",
        "knowledge_chunks",
        "knowledge_chunks_trigram",
        "knowledge_identifiers",
        "knowledge_chunk_meta",
        "knowledge_embeddings",
        "knowledge_symbols",
        "knowledge_references",
        "knowledge_entries",
        "knowledge_resolutions",
        "knowledge_analysis",
        "knowledge_findings",
    ] {
        assert_eq!(table_count(&store, table), 0, "{table} still has rows");
    }
    assert!(store.get_source("gone").unwrap().is_none());
}

#[test]
fn removing_a_source_leaves_other_sources_untouched() {
    let (store, _dir) = populated_store();
    let other = tempdir().unwrap();
    std::fs::write(
        other.path().join("c.rs"),
        "pub fn third_function() -> u8 { 3 }\n",
    )
    .unwrap();
    store
        .add_source("kept", &other.path().to_string_lossy(), &[], &[])
        .unwrap();
    let source = store.get_source("kept").unwrap().unwrap();
    sync_source(&store, &source, None).unwrap();
    let kept_chunks: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            [source.id],
            |row| row.get(0),
        )
        .unwrap();

    store.remove_source("gone").unwrap();

    assert!(store.get_source("kept").unwrap().is_some());
    assert_eq!(table_count(&store, "knowledge_chunk_meta"), kept_chunks);
    assert_eq!(table_count(&store, "knowledge_chunks"), kept_chunks);
    assert!(
        table_count(&store, "knowledge_identifiers") <= kept_chunks,
        "the removed source's identifier rows must be gone"
    );
}
