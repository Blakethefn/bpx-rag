use super::super::*;

#[test]
fn old_embedding_rows_are_migrated_to_their_legacy_model_identity() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE knowledge_embeddings (
            chunk_rowid INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            dim INTEGER NOT NULL DEFAULT 384
         );
         INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
         VALUES (1, X'00000000', 384);",
    )
    .unwrap();

    apply_schema(&conn).unwrap();
    apply_schema(&conn).unwrap();

    let model_id: String = conn
        .query_row(
            "SELECT model_id FROM knowledge_embeddings WHERE chunk_rowid = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(model_id, "sentence-transformers/all-MiniLM-L6-v2");
}

#[test]
fn embedding_insert_update_and_delete_advance_ann_generation() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let generation = || {
        conn.query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };

    assert_eq!(generation(), 0);
    conn.execute(
        "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
         VALUES (1, X'00000000', 1)",
        [],
    )
    .unwrap();
    assert_eq!(generation(), 1);
    conn.execute(
        "UPDATE knowledge_embeddings SET model_id = 'jinaai/jina-embeddings-v2-base-code'
         WHERE chunk_rowid = 1",
        [],
    )
    .unwrap();
    assert_eq!(generation(), 2);
    conn.execute("DELETE FROM knowledge_embeddings WHERE chunk_rowid = 1", [])
        .unwrap();
    assert_eq!(generation(), 3);
}

#[test]
fn upgrading_a_database_file_keeps_every_embedding_row_and_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    {
        let conn = Connection::open(&path).unwrap();
        apply_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO knowledge_chunks (title, content) VALUES ('a.ts', 'function getUserById() {}')",
            [],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, X'0000803F00000040', 2, 'model-x')",
            [rowid],
        )
        .unwrap();
        super::simulate_pre_identifier_index(&conn);
    }
    let snapshot = |conn: &Connection| {
        let embeddings: Vec<(i64, Vec<u8>, i64, String)> = conn
            .prepare("SELECT chunk_rowid, embedding, dim, model_id FROM knowledge_embeddings ORDER BY chunk_rowid")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let generation: i64 = conn
            .query_row(
                "SELECT generation FROM knowledge_embedding_state",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let changes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_embedding_changes",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (embeddings, generation, changes)
    };
    let before = snapshot(&Connection::open(&path).unwrap());

    let conn = Connection::open(&path).unwrap();
    apply_schema(&conn).unwrap();

    assert_eq!(
        snapshot(&conn),
        before,
        "embedding rows or generation changed during upgrade"
    );
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM knowledge_identifiers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 1, "identifier rows must be backfilled");
}
