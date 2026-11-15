use super::super::*;
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;
mod retry;
mod evidence;

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

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn count(store: &KnowledgeStore, sql: &str) -> i64 {
    store.conn().query_row(sql, [], |row| row.get(0)).unwrap()
}
