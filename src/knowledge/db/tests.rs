use super::*;

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
