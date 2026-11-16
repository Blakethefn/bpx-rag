use super::*;

fn pending_metadata_replacements(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM knowledge_chunk_meta_coverage_replace",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn legacy_metadata_replace_tracks_old_and_new_file_coverage() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let first_file = file(&conn, source_id, "src/first.rs");
    let second_file = file(&conn, source_id, "src/second.rs");
    let third_file = file(&conn, source_id, "src/third.rs");
    let moving = chunk(&conn, source_id, first_file);
    let resident = chunk(&conn, source_id, second_file);
    embed(&conn, moving, "model-a", 1);
    embed(&conn, resident, "model-a", 2);

    conn.execute_batch("PRAGMA recursive_triggers = OFF")
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![moving, second_file, source_id],
    )
    .unwrap();
    assert_eq!(count(&conn, first_file, "model-a"), 0);
    assert_eq!(count(&conn, second_file, "model-a"), 2);
    assert_eq!(pending_metadata_replacements(&conn), 0);
    assert_coverage_matches_canonical(&conn);

    conn.execute_batch("PRAGMA recursive_triggers = ON")
        .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![moving, third_file, source_id],
    )
    .unwrap();
    assert_eq!(count(&conn, second_file, "model-a"), 1);
    assert_eq!(count(&conn, third_file, "model-a"), 1);
    assert_eq!(pending_metadata_replacements(&conn), 0);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn nonconflicting_metadata_replace_does_not_reset_existing_file_count() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let existing = chunk(&conn, source_id, file_id);
    embed(&conn, existing, "model-a", 1);

    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('item', 'content')",
        [],
    )
    .unwrap();
    let new_rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, ?2, 1, 'model-a')",
        params![new_rowid, vec![2_u8, 0, 0, 0]],
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![new_rowid, file_id, source_id],
    )
    .unwrap();

    assert_eq!(count(&conn, file_id, "model-a"), 2);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn ignored_metadata_conflict_keeps_counts_correct_and_next_update_cleans_state() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let first_file = file(&conn, source_id, "src/first.rs");
    let second_file = file(&conn, source_id, "src/second.rs");
    let rowid = chunk(&conn, source_id, first_file);
    embed(&conn, rowid, "model-a", 1);

    conn.execute(
        "INSERT OR IGNORE INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id)
         VALUES (?1, ?2, ?3)",
        params![rowid, second_file, source_id],
    )
    .unwrap();
    assert_eq!(count(&conn, first_file, "model-a"), 1);
    assert_eq!(count(&conn, second_file, "model-a"), 0);
    assert_eq!(pending_metadata_replacements(&conn), 1);
    assert_coverage_matches_canonical(&conn);

    conn.execute(
        "UPDATE knowledge_chunk_meta SET metadata = metadata WHERE chunk_rowid = ?1",
        [rowid],
    )
    .unwrap();
    assert_eq!(pending_metadata_replacements(&conn), 0);
    assert_coverage_matches_canonical(&conn);
}
