//! `remove_source` deletes from many tables. A failure part-way must leave the source intact,
//! not metadata with no chunk rows.

use super::super::*;
use crate::knowledge::sync::sync_source;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const TABLES: &[&str] = &[
    "knowledge_sources",
    "knowledge_files",
    "knowledge_chunks",
    "knowledge_chunks_trigram",
    "knowledge_identifiers",
    "knowledge_chunk_meta",
    "knowledge_embeddings",
    "knowledge_embedding_changes",
    "knowledge_symbols",
    "knowledge_references",
    "knowledge_entries",
    "knowledge_resolutions",
    "knowledge_analysis",
    "knowledge_findings",
    "knowledge_identifier_pending",
];

/// Row counts of every table `remove_source` touches, plus the embedding generation.
fn snapshot(store: &KnowledgeStore) -> Vec<(String, i64)> {
    let mut rows: Vec<(String, i64)> = TABLES
        .iter()
        .map(|table| {
            let count = store
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            ((*table).to_string(), count)
        })
        .collect();
    let generation = store
        .conn()
        .query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    rows.push(("generation".to_string(), generation));
    rows
}

/// A store with a source of two files, a vector per chunk, and symbol rows.
fn populated_store() -> (KnowledgeStore, tempfile::TempDir) {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "pub fn first_function() -> u8 { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.rs"),
        "pub fn second_function() -> u8 { 2 }\n",
    )
    .unwrap();
    store
        .add_source("gone", &dir.path().to_string_lossy(), &[], &[])
        .unwrap();
    let source = store.get_source("gone").unwrap().unwrap();
    sync_source(&store, &source, None).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             SELECT chunk_rowid, X'00000000', 1, 'test-model' FROM knowledge_chunk_meta",
            [],
        )
        .unwrap();
    (store, dir)
}

fn table_count(store: &KnowledgeStore, table: &str) -> i64 {
    store
        .conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn a_failure_at_any_step_leaves_the_source_intact() {
    let (store, _dir) = populated_store();
    let chunks = table_count(&store, "knowledge_chunk_meta");
    assert!(chunks >= 2, "the fixture needs several chunks");
    assert!(table_count(&store, "knowledge_symbols") >= 2);
    let before = snapshot(&store);
    // One step per chunk, then the symbol, metadata, and source deletes.
    let steps = chunks as usize + 3;

    for fail_after in 1..=steps {
        REMOVE_FAILPOINT.with(|point| point.set(Some(fail_after)));
        let error = store
            .remove_source("gone")
            .expect_err("the injected failure must surface");
        assert!(
            error.to_string().contains("injected failure"),
            "step {fail_after}: {error}"
        );
        assert_eq!(
            snapshot(&store),
            before,
            "step {fail_after} left a partially removed source"
        );
        assert!(store.get_source("gone").unwrap().is_some());
    }
    REMOVE_FAILPOINT.with(|point| point.set(None));

    let integrity: String = store
        .conn()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

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

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, label: &str, dir: &Path) {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
}

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn count(store: &KnowledgeStore, sql: &str) -> i64 {
    store.conn().query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn removing_a_source_drops_evidence_resolutions_analysis_and_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\nfn unused() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_references") > 0);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_resolutions") > 0);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_analysis"), 1);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_findings") > 0);

    store.remove_source("demo").unwrap();

    for table in [
        "knowledge_references",
        "knowledge_entries",
        "knowledge_resolutions",
        "knowledge_analysis",
        "knowledge_findings",
        "knowledge_symbols",
    ] {
        assert_eq!(
            count(&store, &format!("SELECT COUNT(*) FROM {table}")),
            0,
            "{table} still has rows"
        );
    }
}
