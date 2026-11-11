use super::super::super::*;
use super::legacy;
use super::{count, identifier_row, set_version_one};
use crate::knowledge::schema::apply_schema;

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
