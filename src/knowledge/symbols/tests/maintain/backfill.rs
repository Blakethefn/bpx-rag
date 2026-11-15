use super::super::super::*;
use super::super::{count, exact, hit, lookup_with, sync};
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn removing_a_source_deletes_its_symbol_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn gone_with_source() {}\n").unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM knowledge_symbols ks JOIN knowledge_sources s ON s.id = ks.source_id WHERE s.label = ?1",
            "test"
        ),
        1
    );

    store.remove_source("test").unwrap();

    let total: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_symbols", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(total, 0);
}

#[test]
fn existing_index_is_backfilled_on_sync_without_rechunking() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn legacy_entry() {}\n").unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");

    store
        .conn()
        .execute_batch(
            "DELETE FROM knowledge_symbols;
             UPDATE knowledge_files SET symbol_version = 0;",
        )
        .unwrap();
    let chunk_rows_before: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };

    let pending = lookup_with(&store, "legacy_entry", MatchMode::Exact, None, 20);
    assert!(pending.hits.is_empty());
    assert_eq!(
        pending.pending,
        vec![PendingSymbols {
            source_label: "test".to_string(),
            files: 1
        }]
    );

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        result.files_updated, 0,
        "backfill must not re-chunk the file"
    );
    assert_eq!(result.files_unchanged, 1);
    assert_eq!(result.chunks_indexed, 0);

    let chunk_rows_after: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(chunk_rows_before, chunk_rows_after);

    let after = lookup_with(&store, "legacy_entry", MatchMode::Exact, None, 20);
    assert_eq!(after.hits.len(), 1);
    assert!(after.pending.is_empty());
}
