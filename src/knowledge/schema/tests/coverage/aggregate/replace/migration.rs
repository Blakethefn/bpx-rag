use rusqlite::{Connection, OptionalExtension};

#[test]
fn interrupted_coverage_backfill_rolls_back_partial_rows_ddl_and_marker_then_retries() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE knowledge_index_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE knowledge_files (id INTEGER PRIMARY KEY);
         CREATE TABLE knowledge_chunk_meta (chunk_rowid INTEGER PRIMARY KEY, file_id INTEGER);
         CREATE TABLE knowledge_embeddings (
            chunk_rowid INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            dim INTEGER NOT NULL,
            model_id TEXT NOT NULL
         );
         CREATE TABLE knowledge_embedding_coverage (
            file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            model_id TEXT NOT NULL,
            embedded_count INTEGER NOT NULL CHECK (embedded_count >= 0),
            PRIMARY KEY (file_id, model_id)
         );
         CREATE TABLE migration_failure_state (insert_attempts INTEGER NOT NULL);
         INSERT INTO migration_failure_state VALUES (0);
         INSERT INTO knowledge_files (id) VALUES (1), (2);
         INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id) VALUES (1, 1), (2, 2);
         INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (1, X'00', 1, 'model-a'), (2, X'00', 1, 'model-a');
         CREATE TRIGGER injected_backfill_failure
         BEFORE INSERT ON knowledge_embedding_coverage BEGIN
            UPDATE migration_failure_state SET insert_attempts = insert_attempts + 1;
            SELECT CASE WHEN (SELECT insert_attempts FROM migration_failure_state) = 2
                THEN RAISE(ABORT, 'injected failure after first coverage row')
            END;
         END;",
    )
    .unwrap();

    let result = crate::knowledge::schema::migrate::ensure_embedding_coverage(&conn);
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("injected failure after first coverage row"));
    let aggregate_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_embedding_coverage",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(aggregate_count, 0);
    let attempts: i64 = conn
        .query_row(
            "SELECT insert_attempts FROM migration_failure_state",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 0, "backfill statement changes must roll back");
    for table in [
        "knowledge_embedding_coverage_replace",
        "knowledge_chunk_meta_coverage_replace",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists, "failed migration left table {table}");
    }
    for index in ["idx_embedding_coverage_model_file", "idx_embeddings_model"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1)",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists, "failed migration left index {index}");
    }
    let marker: Option<String> = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key='embedding_coverage_v1'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(marker, None, "failed migration left its marker");

    conn.execute_batch(
        "DROP TRIGGER injected_backfill_failure; DROP TABLE migration_failure_state",
    )
    .unwrap();
    crate::knowledge::schema::migrate::ensure_embedding_coverage(&conn).unwrap();
    let coverage: i64 = conn
        .query_row(
            "SELECT SUM(embedded_count) FROM knowledge_embedding_coverage",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(coverage, 2, "a retry should rebuild both file/model rows");
    let marker: Option<String> = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key='embedding_coverage_v1'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(marker.as_deref(), Some("3"));
}
