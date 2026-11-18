use super::super::*;
use rusqlite::{Connection, params};

fn schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    apply_schema(&conn).unwrap();
    conn
}

fn source(conn: &Connection, label: &str) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_sources(label, path) VALUES (?1, ?2)",
        params![label, format!("/{label}")],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn file(conn: &Connection, source_id: i64, bytes: i64) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_files(source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, 'file.rs', 'hash', ?2)",
        params![source_id, bytes],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn chunk(conn: &Connection, source_id: i64, file_id: i64) -> i64 {
    conn.execute("INSERT INTO knowledge_chunks(title, content) VALUES ('title', 'body')", [])
        .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta(chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![rowid, file_id, source_id],
    )
    .unwrap();
    rowid
}

fn stats(conn: &Connection, source_id: i64) -> (i64, i64, i64, Option<String>) {
    conn.query_row(
        "SELECT chunk_count, embedded_count, total_bytes, last_indexed_at
         FROM source_stats WHERE source_id = ?1",
        [source_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .unwrap()
}

#[test]
fn source_stats_follow_chunk_embedding_and_file_mutations() {
    let conn = schema();
    let first = source(&conn, "first");
    let second = source(&conn, "second");
    let file_id = file(&conn, first, 17);
    let chunk_id = chunk(&conn, first, file_id);
    assert_eq!(stats(&conn, first).0, 1);
    assert_eq!(stats(&conn, first).1, 0);
    assert_eq!(stats(&conn, first).2, 17);
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_chunk_meta(chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![chunk_id, file_id, first],
    )
    .unwrap();
    assert_eq!(stats(&conn, first).0, 1);
    assert_eq!(stats(&conn, first).2, 17, "metadata replace changed file bytes");

    conn.execute(
        "INSERT INTO knowledge_embeddings(chunk_rowid, embedding, dim, model_id)
         VALUES (?1, X'00000000', 1, 'model-a')",
        [chunk_id],
    )
    .unwrap();
    assert_eq!(stats(&conn, first).1, 1);
    assert_eq!(stats(&conn, first).2, 17, "embedding insert changed file bytes");
    conn.execute_batch("PRAGMA recursive_triggers = ON").unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings(chunk_rowid, embedding, dim, model_id)
         VALUES (?1, X'01010101', 1, 'model-a')",
        [chunk_id],
    )
    .unwrap();
    assert_eq!(stats(&conn, first).1, 1);
    assert_eq!(stats(&conn, first).2, 17, "embedding replace changed file bytes");

    conn.execute(
        "UPDATE knowledge_files SET size_bytes = 23, indexed_at = '2030-01-01 00:00:00'
         WHERE id = ?1",
        [file_id],
    )
    .unwrap();
    assert_eq!(stats(&conn, first).2, 23);
    assert_eq!(stats(&conn, first).3.as_deref(), Some("2030-01-01 00:00:00"));

    conn.execute(
        "UPDATE knowledge_chunk_meta SET source_id = ?1 WHERE chunk_rowid = ?2",
        params![second, chunk_id],
    )
    .unwrap();
    assert_eq!(stats(&conn, first).0, 0);
    assert_eq!(stats(&conn, first).1, 0);
    assert_eq!(stats(&conn, second).0, 1);
    assert_eq!(stats(&conn, second).1, 1);

    conn.execute("DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1", [chunk_id])
        .unwrap();
    assert_eq!(stats(&conn, second).1, 0);
    conn.execute("DELETE FROM knowledge_chunk_meta WHERE chunk_rowid = ?1", [chunk_id])
        .unwrap();
    assert_eq!(stats(&conn, second).0, 0);

    conn.execute("DELETE FROM knowledge_files WHERE id = ?1", [file_id])
        .unwrap();
    assert_eq!(stats(&conn, first).2, 0);
    assert_eq!(stats(&conn, first).3, None);
}

#[test]
fn source_stats_metadata_replace_stays_accurate_in_both_trigger_modes() {
    for recursive_triggers in [false, true] {
        let conn = schema();
        let source_id = source(&conn, "replace");
        let file_id = file(&conn, source_id, 17);
        let chunk_id = chunk(&conn, source_id, file_id);
        conn.execute_batch(if recursive_triggers {
            "PRAGMA recursive_triggers = ON"
        } else {
            "PRAGMA recursive_triggers = OFF"
        })
        .unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO knowledge_chunk_meta(chunk_rowid, file_id, source_id)
             VALUES (?1, ?2, ?3)",
            params![chunk_id, file_id, source_id],
        )
        .unwrap();

        assert_eq!(stats(&conn, source_id).0, 1);
        let pending: i64 = conn
            .query_row("SELECT COUNT(*) FROM source_stats_meta_replace", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(pending, 0);
    }
}

#[test]
fn source_stats_migration_backfills_existing_sources() {
    let conn = schema();
    conn.execute_batch(
        "DROP TRIGGER source_stats_source_insert;
         DROP TRIGGER source_stats_file_insert;
         DROP TRIGGER source_stats_file_update;
         DROP TRIGGER source_stats_file_delete;
         DROP TRIGGER source_stats_meta_prepare_replace;
         DROP TRIGGER source_stats_meta_insert;
         DROP TRIGGER source_stats_meta_delete;
         DROP TRIGGER source_stats_meta_move;
         DROP TRIGGER source_stats_embedding_prepare_replace;
         DROP TRIGGER source_stats_embedding_insert;
         DROP TRIGGER source_stats_embedding_delete;
         DELETE FROM source_stats;
         DELETE FROM knowledge_index_meta WHERE key = 'source_stats_v1';",
    )
    .unwrap();
    let source_id = source(&conn, "legacy");
    let file_id = file(&conn, source_id, 41);
    let chunk_id = chunk(&conn, source_id, file_id);
    conn.execute(
        "INSERT INTO knowledge_embeddings(chunk_rowid, embedding, dim, model_id)
         VALUES (?1, X'00000000', 1, 'model-a')",
        [chunk_id],
    )
    .unwrap();

    migrate::ensure_source_stats(&conn).unwrap();

    assert_eq!(stats(&conn, source_id).0, 1);
    assert_eq!(stats(&conn, source_id).1, 1);
    assert_eq!(stats(&conn, source_id).2, 41);
}
