//! Sync against a database that a pre-identifier writer has modified.

use super::super::*;
use crate::knowledge::identifiers::identifier_words;
use crate::knowledge::identifiers::tests::maintenance::legacy;
use tempfile::tempdir;

use crate::support::{add_source, make_store};

/// Every identifier row belongs to a live chunk, and its words match the stored chunk text.
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

    // A pre-identifier writer deletes the file and adds a chunk, which reuses its rowid.
    legacy::delete_file(store.conn(), billing_id);
    legacy::insert_chunk(store.conn(), 9_001, source.id, "orphan.md", "plain prose");

    // The current binary then syncs, adding a file. This used to fail with SQLite
    // constraint 1555 because the reused rowid still carried stale identifier words.
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

fn symbol_hits(store: &KnowledgeStore, name: &str) -> Vec<(u32, u32)> {
    let report = crate::knowledge::symbols::lookup(
        store,
        &crate::knowledge::symbols::SymbolQuery {
            name,
            mode: crate::knowledge::symbols::MatchMode::Exact,
            source: None,
            limit: 10,
        },
    )
    .unwrap();
    report
        .hits
        .iter()
        .map(|hit| (hit.line_start, hit.line_end))
        .collect()
}

#[test]
fn legacy_content_update_hides_stale_symbol_lines_until_sync_rebuilds_them() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha_one() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    let source = add_source(&store, "identifier-maintenance", dir.path());
    sync_source(&store, &source, None).unwrap();
    assert_eq!(symbol_hits(&store, "alpha_one"), vec![(1, 3)]);
    let file_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE rel_path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // A pre-symbol writer replaces the chunks and stores the new hash. It never writes symbols.
    let updated = "\n\npub fn beta_two() -> u8 {\n    2\n}\n";
    fs::write(dir.path().join("lib.rs"), updated).unwrap();
    legacy::delete_chunks(store.conn(), file_id);
    legacy::insert_chunk(store.conn(), file_id, source.id, "lib.rs", updated);
    let hash = crate::knowledge::sync::sha256_hex(updated.as_bytes());
    legacy::set_content_hash(store.conn(), file_id, &hash);

    assert!(
        symbol_hits(&store, "alpha_one").is_empty(),
        "a line range from the previous content was cited"
    );
    assert!(
        symbol_hits(&store, "beta_two").is_empty(),
        "not indexed yet"
    );

    sync_source(&store, &source, None).unwrap();

    assert!(symbol_hits(&store, "alpha_one").is_empty());
    assert_eq!(symbol_hits(&store, "beta_two"), vec![(3, 5)]);
    assert_identifier_index_matches_chunks(&store);
}
