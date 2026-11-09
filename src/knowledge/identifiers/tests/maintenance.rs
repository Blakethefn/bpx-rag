//! Identifier-index maintenance under legacy writers.
//!
//! The `legacy` helpers repeat, statement for statement, the chunk writes that `master`
//! performs. A build that predates the identifier index runs exactly these statements, and
//! the database triggers must keep the identifier table consistent for them too.

use super::super::*;
use crate::knowledge::schema::apply_schema;
use rusqlite::Connection;

pub(crate) mod legacy {
    use rusqlite::{params, Connection};

    /// Mirror of `master`'s `index_prepared_file` chunk writes: content row, trigram row,
    /// then metadata row. No identifier row is written.
    pub(crate) fn insert_chunk(
        conn: &Connection,
        file_id: i64,
        source_id: i64,
        title: &str,
        content: &str,
    ) -> i64 {
        conn.execute(
            "INSERT OR IGNORE INTO knowledge_sources (id, label, path) VALUES (?1, ?2, '/unused')",
            params![source_id, format!("source-{source_id}")],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO knowledge_files (id, source_id, rel_path, content_hash, size_bytes)
             VALUES (?1, ?2, ?3, 'hash', 1)",
            params![file_id, source_id, format!("file-{file_id}.txt")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO knowledge_chunks(title, content) VALUES (?1, ?2)",
            params![title, content],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO knowledge_chunks_trigram(rowid, title, content) VALUES (?1, ?2, ?3)",
            params![rowid, title, content],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO knowledge_chunk_meta
                (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
             VALUES (?1, ?2, ?3, 1, 3, '{}')",
            params![rowid, file_id, source_id],
        )
        .unwrap();
        rowid
    }

    /// Mirror of `master`'s `delete_file_chunks`: chunk, trigram, embedding, and metadata rows.
    pub(crate) fn delete_chunks(conn: &Connection, file_id: i64) {
        let rowids: Vec<i64> = {
            let mut statement = conn
                .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1")
                .unwrap();
            statement
                .query_map(params![file_id], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        for rowid in &rowids {
            conn.execute(
                "DELETE FROM knowledge_chunks WHERE rowid = ?1",
                params![rowid],
            )
            .unwrap();
            conn.execute(
                "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
                params![rowid],
            )
            .unwrap();
        }
        conn.execute(
            "DELETE FROM knowledge_embeddings WHERE chunk_rowid IN
             (SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1)",
            params![file_id],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM knowledge_chunk_meta WHERE file_id = ?1",
            params![file_id],
        )
        .unwrap();
    }

    /// Mirror of `master`'s `delete_file_chunks` followed by its file-row delete.
    pub(crate) fn delete_file(conn: &Connection, file_id: i64) {
        delete_chunks(conn, file_id);
        conn.execute(
            "DELETE FROM knowledge_files WHERE id = ?1",
            params![file_id],
        )
        .unwrap();
    }

    /// Mirror of `master`'s update path for a changed file: it stores the new hash on the
    /// file row, which keeps its id, and replaces its chunks.
    pub(crate) fn set_content_hash(conn: &Connection, file_id: i64, hash: &str) {
        conn.execute(
            "UPDATE knowledge_files SET content_hash = ?1 WHERE id = ?2",
            params![hash, file_id],
        )
        .unwrap();
    }

    /// Mirror of `master`'s `remove_source` chunk deletes.
    pub(crate) fn remove_source(conn: &Connection, source_id: i64) {
        let rowids: Vec<i64> = {
            let mut statement = conn
                .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE source_id = ?1")
                .unwrap();
            statement
                .query_map(params![source_id], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        for rowid in &rowids {
            conn.execute(
                "DELETE FROM knowledge_chunks WHERE rowid = ?1",
                params![rowid],
            )
            .unwrap();
            conn.execute(
                "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
                params![rowid],
            )
            .unwrap();
            conn.execute(
                "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
                params![rowid],
            )
            .unwrap();
        }
        conn.execute(
            "DELETE FROM knowledge_chunk_meta WHERE source_id = ?1",
            params![source_id],
        )
        .unwrap();
    }
}

fn identifier_row(conn: &Connection, rowid: i64) -> Option<String> {
    conn.query_row(
        "SELECT words FROM knowledge_identifiers WHERE rowid = ?1",
        params![rowid],
        |row| row.get(0),
    )
    .ok()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// Simulate a version-1 database (the branch's earlier identifier index) with residue.
fn set_version_one(conn: &Connection) {
    conn.execute(
        "UPDATE knowledge_index_meta SET value = '1' WHERE key = 'identifier_index_version'",
        [],
    )
    .unwrap();
}

#[test]
fn legacy_insert_is_queued_and_indexed_on_next_open() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let rowid = legacy::insert_chunk(&conn, 1, 1, "users.ts", "export function getUserById() {}");

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        1
    );
    assert_eq!(identifier_row(&conn, rowid), None);

    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, rowid).as_deref(),
        Some("get user by id")
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
}

#[test]
fn legacy_delete_removes_the_identifier_row_of_its_chunks() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let rowid = legacy::insert_chunk(&conn, 7, 1, "users.ts", "function getUserById() {}");
    ensure_identifier_index(&conn).unwrap();
    assert!(identifier_row(&conn, rowid).is_some());

    legacy::delete_file(&conn, 7);

    assert_eq!(
        identifier_row(&conn, rowid),
        None,
        "orphaned identifier row"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        0
    );
}

#[test]
fn legacy_source_removal_clears_identifier_rows() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    legacy::insert_chunk(&conn, 1, 9, "a.ts", "function getUserById() {}");
    legacy::insert_chunk(&conn, 2, 9, "b.ts", "function parseHTTPResponse() {}");
    ensure_identifier_index(&conn).unwrap();

    legacy::remove_source(&conn, 9);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        0
    );
}

#[test]
fn reused_rowid_does_not_inherit_the_words_of_a_deleted_chunk() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    // The table is empty, so the next insert reuses the deleted rowid.
    let reused = legacy::insert_chunk(&conn, 2, 1, "notes.md", "pizza dough and a hot oven");
    assert_eq!(reused, old, "test requires a reused rowid");
    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, reused).as_deref(),
        Some(""),
        "a plain-prose chunk must not carry the deleted chunk's identifier words"
    );
}

#[test]
fn reused_rowid_with_a_compound_identifier_gets_its_own_words() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    let reused = legacy::insert_chunk(&conn, 2, 1, "users.ts", "function getUserById() {}");
    assert_eq!(reused, old);
    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, reused).as_deref(),
        Some("get user by id")
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        1
    );
}

#[test]
fn version_one_residue_is_repaired_by_the_rebuild() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let live = legacy::insert_chunk(&conn, 1, 1, "users.ts", "function getUserById() {}");
    ensure_identifier_index(&conn).unwrap();
    // Residue a version-1 writer could leave: a row for a chunk that no longer exists, and
    // stale words attached to a live rowid.
    conn.execute(
        "INSERT INTO knowledge_identifiers(rowid, words) VALUES (999, 'build invoice total')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE knowledge_identifiers SET words = 'build invoice total' WHERE rowid = ?1",
        params![live],
    )
    .unwrap();
    set_version_one(&conn);

    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, 999),
        None,
        "orphan survived the rebuild"
    );
    assert_eq!(
        identifier_row(&conn, live).as_deref(),
        Some("get user by id")
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        1
    );
    let version: String = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key = 'identifier_index_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, IDENTIFIER_INDEX_VERSION.to_string());
}

#[test]
fn interrupted_rebuild_rolls_back_completely() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    let conn = Connection::open(&path).unwrap();
    apply_schema(&conn).unwrap();
    for index in 0..5 {
        legacy::insert_chunk(
            &conn,
            index,
            1,
            "a.ts",
            &format!("function getUserNumber{index}() {{}}"),
        );
    }
    ensure_identifier_index(&conn).unwrap();
    let before = count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers");
    conn.execute(
        "INSERT INTO knowledge_identifiers(rowid, words) VALUES (999, 'orphan')",
        [],
    )
    .unwrap();
    set_version_one(&conn);

    REBUILD_FAILPOINT.with(|point| point.set(Some(2)));
    let result = ensure_identifier_index(&conn);
    REBUILD_FAILPOINT.with(|point| point.set(None));

    assert!(result.is_err(), "the injected failure must surface");
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        before + 1,
        "a failed rebuild must not change identifier rows"
    );
    assert_eq!(identifier_row(&conn, 999).as_deref(), Some("orphan"));
    let version: String = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key = 'identifier_index_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        version, "1",
        "version must not advance after a failed rebuild"
    );

    // A later open completes the migration.
    ensure_identifier_index(&conn).unwrap();
    assert_eq!(identifier_row(&conn, 999), None);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        before
    );
}

#[test]
fn repeated_migration_is_idempotent_and_keeps_embeddings() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let rowid = legacy::insert_chunk(&conn, 1, 1, "a.ts", "function getUserById() {}");
    conn.execute(
        "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, X'0000803F00000040', 2, 'model-x')",
        params![rowid],
    )
    .unwrap();
    let generation_before: i64 = count(&conn, "SELECT generation FROM knowledge_embedding_state");

    set_version_one(&conn);
    apply_schema(&conn).unwrap();
    apply_schema(&conn).unwrap();

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        1
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM knowledge_embeddings"), 1);
    assert_eq!(
        count(&conn, "SELECT generation FROM knowledge_embedding_state"),
        generation_before,
        "identifier maintenance must not change the embedding generation"
    );
}

#[test]
fn rolled_back_legacy_write_leaves_no_pending_entry() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    legacy::insert_chunk(&conn, 1, 1, "a.ts", "function getUserById() {}");
    conn.execute_batch("ROLLBACK").unwrap();

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM knowledge_chunk_meta"), 0);
}

#[test]
fn pending_entry_without_metadata_is_dropped_not_indexed() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO knowledge_identifier_pending (chunk_rowid) VALUES (42)",
        [],
    )
    .unwrap();

    ensure_identifier_index(&conn).unwrap();

    assert_eq!(identifier_row(&conn, 42), None);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
}

#[test]
fn deleted_chunk_leaves_no_identifier_row_for_its_rowid_before_any_reopen() {
    // A long-running process does not reopen the database, so nothing reprocesses the queue
    // while it runs. The identifier table itself must be clean right after a legacy delete.
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    assert_eq!(identifier_row(&conn, old), None);
    let reused = legacy::insert_chunk(&conn, 2, 1, "notes.md", "pizza dough");
    assert_eq!(reused, old);
    assert_eq!(
        identifier_row(&conn, reused),
        None,
        "the reused rowid must not carry the deleted chunk's words before the queue runs"
    );
}
