use super::super::*;

mod migration;
mod updates;

fn pending_replacements(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM knowledge_embedding_coverage_replace",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn legacy_replace_embedding_write_keeps_coverage_consistent_with_both_trigger_modes() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);

    // Older binaries used INSERT OR REPLACE. Exercise both modes because SQLite
    // runs the implicit DELETE trigger only when recursive triggers are enabled.
    conn.execute_batch("PRAGMA recursive_triggers = OFF")
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-b')",
        params![rowid, vec![2_u8, 0, 0, 0]],
    )
    .unwrap();
    assert_eq!(count(&conn, file_id, "model-a"), 0);
    assert_eq!(count(&conn, file_id, "model-b"), 1);
    assert_coverage_matches_canonical(&conn);

    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-b')",
        params![rowid, vec![3_u8, 0, 0, 0]],
    )
    .unwrap();
    assert_eq!(count(&conn, file_id, "model-b"), 1);
    assert_coverage_matches_canonical(&conn);

    conn.execute_batch("PRAGMA recursive_triggers = ON")
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-a')",
        params![rowid, vec![4_u8, 0, 0, 0]],
    )
    .unwrap();
    assert_eq!(count(&conn, file_id, "model-a"), 1);
    assert_eq!(count(&conn, file_id, "model-b"), 0);
    assert_coverage_matches_canonical(&conn);

    apply_schema(&conn).unwrap();
    assert_coverage_matches_canonical(&conn);
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_embedding_coverage_replace",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn current_upsert_clears_replace_state_and_tracks_model_changes_once() {
    let conn = schema();
    let source_id = source(&conn, "modern-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);
    embed(&conn, rowid, "model-a", 2);
    assert_eq!(count(&conn, file_id, "model-a"), 1);

    embed(&conn, rowid, "model-b", 3);
    assert_eq!(count(&conn, file_id, "model-a"), 0);
    assert_eq!(count(&conn, file_id, "model-b"), 1);
    assert_coverage_matches_canonical(&conn);
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_embedding_coverage_replace",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn legacy_replace_insert_of_a_new_embedding_increments_existing_aggregate() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let first = chunk(&conn, source_id, file_id);
    let second = chunk(&conn, source_id, file_id);
    embed(&conn, first, "model-a", 1);

    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-a')",
        params![second, vec![2_u8, 0, 0, 0]],
    )
    .unwrap();
    assert_eq!(count(&conn, file_id, "model-a"), 2);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn ignored_embedding_conflict_keeps_counts_correct_and_next_update_cleans_state() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);

    conn.execute(
        "INSERT OR IGNORE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-b')",
        params![rowid, vec![2_u8, 0, 0, 0]],
    )
    .unwrap();

    assert_eq!(count(&conn, file_id, "model-a"), 1);
    assert_eq!(count(&conn, file_id, "model-b"), 0);
    assert_coverage_matches_canonical(&conn);
    assert_eq!(pending_replacements(&conn), 1);

    conn.execute(
        "UPDATE knowledge_embeddings SET embedding = embedding WHERE chunk_rowid = ?1",
        [rowid],
    )
    .unwrap();
    assert_eq!(pending_replacements(&conn), 0);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn missing_replace_state_table_rebuilds_coverage_and_restores_it() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);
    conn.execute_batch("DROP TABLE knowledge_embedding_coverage_replace")
        .unwrap();

    apply_schema(&conn).unwrap();
    assert_coverage_matches_canonical(&conn);
    conn.execute_batch("PRAGMA recursive_triggers = OFF")
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_embeddings
            (chunk_rowid, embedding, dim, model_id) VALUES (?1, ?2, 1, 'model-b')",
        params![rowid, vec![3_u8, 0, 0, 0]],
    )
    .unwrap();
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn migration_rebuilds_coverage_drift_from_the_previous_trigger_version() {
    let conn = schema();
    let source_id = source(&conn, "legacy-aggregate");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let rowid = chunk(&conn, source_id, file_id);
    embed(&conn, rowid, "model-a", 1);
    conn.execute(
        "UPDATE knowledge_embedding_coverage SET embedded_count = 4 WHERE file_id = ?1",
        [file_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE knowledge_index_meta SET value = '2' WHERE key = 'embedding_coverage_v1'",
        [],
    )
    .unwrap();

    apply_schema(&conn).unwrap();
    assert_coverage_matches_canonical(&conn);
}
