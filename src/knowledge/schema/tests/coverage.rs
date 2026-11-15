use super::super::*;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

mod aggregate;

fn schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    apply_schema(&conn).unwrap();
    conn
}

fn source(conn: &Connection, label: &str) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_sources (label, path) VALUES (?1, ?2)",
        params![label, format!("/{label}")],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn file(conn: &Connection, source_id: i64, path: &str) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, ?2, 'hash', 1)",
        params![source_id, path],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn chunk(conn: &Connection, source_id: i64, file_id: i64) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('item', 'content')",
        [],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![rowid, file_id, source_id],
    )
    .unwrap();
    rowid
}

fn embed(conn: &Connection, rowid: i64, model: &str, value: u8) {
    conn.execute(
        "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(chunk_rowid) DO UPDATE SET embedding = excluded.embedding,
             dim = excluded.dim, model_id = excluded.model_id",
        params![rowid, vec![value, 0, 0, 0], model],
    )
    .unwrap();
}

fn coverage(conn: &Connection) -> BTreeMap<(i64, String), i64> {
    conn.prepare(
        "SELECT file_id, model_id, embedded_count
         FROM knowledge_embedding_coverage WHERE embedded_count > 0
         ORDER BY file_id, model_id",
    )
    .unwrap()
    .query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn count(conn: &Connection, file_id: i64, model: &str) -> i64 {
    coverage(conn)
        .get(&(file_id, model.to_string()))
        .copied()
        .unwrap_or_default()
}

fn canonical_coverage(conn: &Connection) -> BTreeMap<(i64, String), i64> {
    conn.prepare(
        "SELECT kcm.file_id, e.model_id, COUNT(*)
         FROM knowledge_chunk_meta kcm
         JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid
         GROUP BY kcm.file_id, e.model_id ORDER BY kcm.file_id, e.model_id",
    )
    .unwrap()
    .query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn assert_coverage_matches_canonical(conn: &Connection) {
    assert_eq!(coverage(conn), canonical_coverage(conn));
}

fn coverage_sum(conn: &Connection, source_id: i64, path_filter: &str) -> i64 {
    conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(c.embedded_count), 0)
             FROM knowledge_embedding_coverage c
             JOIN knowledge_files f ON f.id = c.file_id
             WHERE f.source_id = ?1 AND c.model_id = 'model-a' {path_filter}"
        ),
        [source_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn canonical_sum(conn: &Connection, source_id: i64, path_filter: &str) -> i64 {
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM knowledge_chunk_meta kcm
             JOIN knowledge_files f ON f.id = kcm.file_id
             JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid
             WHERE kcm.source_id = ?1 AND e.model_id = 'model-a' {path_filter}"
        ),
        [source_id],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn schema_backfills_legacy_embeddings_once_and_reapplies_idempotently() {
    let conn = schema();
    let source_id = source(&conn, "legacy");
    let file_id = file(&conn, source_id, "src/legacy.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);

    let triggers: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'trigger' AND sql LIKE '%knowledge_embedding_coverage%'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for trigger in triggers {
        conn.execute_batch(&format!("DROP TRIGGER IF EXISTS \"{trigger}\""))
            .unwrap();
    }
    conn.execute_batch("DROP TABLE knowledge_embedding_coverage")
        .unwrap();

    apply_schema(&conn).unwrap();
    assert_eq!(
        coverage(&conn),
        BTreeMap::from([((file_id, "model-a".into()), 1)])
    );
    apply_schema(&conn).unwrap();
    assert_eq!(
        coverage(&conn),
        BTreeMap::from([((file_id, "model-a".into()), 1)])
    );
}

#[test]
fn embedding_changes_adjust_per_file_model_counts_without_double_counting() {
    let conn = schema();
    let source_id = source(&conn, "repo");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let first = chunk(&conn, source_id, file_id);
    let second = chunk(&conn, source_id, file_id);

    embed(&conn, first, "model-a", 1);
    embed(&conn, second, "model-a", 2);
    assert_eq!(count(&conn, file_id, "model-a"), 2);
    embed(&conn, first, "model-a", 3);
    assert_eq!(count(&conn, file_id, "model-a"), 2);

    embed(&conn, first, "model-b", 4);
    assert_eq!(count(&conn, file_id, "model-a"), 1);
    assert_eq!(count(&conn, file_id, "model-b"), 1);
    conn.execute(
        "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
        [second],
    )
    .unwrap();
    assert_eq!(count(&conn, file_id, "model-a"), 0);
    assert_coverage_matches_canonical(&conn);
}
