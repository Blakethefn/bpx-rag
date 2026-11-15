use super::super::*;
use crate::knowledge::identifiers::identifier_words;
use crate::knowledge::identifiers::tests::maintenance::legacy;
use tempfile::tempdir;

mod symbols;

use crate::support::{add_source, make_store};

fn assert_identifier_index_matches_chunks(store: &KnowledgeStore) {
    let orphans: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_identifiers
             WHERE rowid NOT IN (SELECT chunk_rowid FROM knowledge_chunk_meta)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0, "identifier rows without a live chunk");

    let mut statement = store
        .conn()
        .prepare(
            "SELECT m.chunk_rowid, c.title, c.content, i.words
             FROM knowledge_chunk_meta m
             JOIN knowledge_chunks c ON c.rowid = m.chunk_rowid
             LEFT JOIN knowledge_identifiers i ON i.rowid = m.chunk_rowid",
        )
        .unwrap();
    let rows: Vec<(i64, String, String, Option<String>)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(!rows.is_empty());
    for (rowid, title, content, words) in rows {
        let expected = identifier_words(&format!("{title} {content}"));
        assert_eq!(words.as_deref(), Some(expected.as_str()), "chunk {rowid}");
    }
}

#[test]
fn sync_succeeds_after_a_legacy_writer_deletes_and_reuses_chunk_rowids() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("billing.ts"),
        "export function buildInvoiceTotal() { return 1; }\n",
    )
    .unwrap();
    let source = add_source(&store, "identifier-maintenance", dir.path());
    sync_source(&store, &source, None).unwrap();
    let billing_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE rel_path = 'billing.ts'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    legacy::delete_file(store.conn(), billing_id);
    legacy::insert_chunk(store.conn(), 9_001, source.id, "orphan.md", "plain prose");

    fs::write(
        dir.path().join("users.ts"),
        "export function getUserById() {}\n",
    )
    .unwrap();
    let result = sync_source(&store, &source, None);

    assert!(result.is_ok(), "sync failed: {:?}", result.err());
    assert_identifier_index_matches_chunks(&store);
}

#[test]
fn legacy_deletion_then_sync_leaves_no_stale_identifier_citations() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("billing.ts"),
        "export function buildInvoiceTotal() { return 1; }\n",
    )
    .unwrap();
    let source = add_source(&store, "identifier-maintenance", dir.path());
    sync_source(&store, &source, None).unwrap();
    let billing_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE rel_path = 'billing.ts'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    legacy::delete_file(store.conn(), billing_id);
    fs::write(dir.path().join("pizza.md"), "pizza dough and a hot oven\n").unwrap();
    sync_source(&store, &source, None).unwrap();

    let stale: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_identifiers i
             JOIN knowledge_chunks c ON c.rowid = i.rowid
             WHERE i.words LIKE '%invoice%' AND c.content NOT LIKE '%invoice%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stale, 0,
        "a chunk inherited another chunk's identifier words"
    );
    assert_identifier_index_matches_chunks(&store);
}
