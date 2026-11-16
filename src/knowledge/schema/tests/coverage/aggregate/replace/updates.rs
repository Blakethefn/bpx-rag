use super::super::*;

#[test]
fn update_or_replace_embedding_model_change_preserves_other_model_counts() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let first = chunk(&conn, source_id, file_id);
    let second = chunk(&conn, source_id, file_id);
    embed(&conn, first, "model-a", 1);
    embed(&conn, second, "model-b", 2);

    conn.execute(
        "UPDATE OR REPLACE knowledge_embeddings SET model_id = 'model-b'
         WHERE chunk_rowid = ?1",
        [first],
    )
    .unwrap();

    assert_eq!(count(&conn, file_id, "model-a"), 0);
    assert_eq!(count(&conn, file_id, "model-b"), 2);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn update_or_replace_metadata_move_preserves_existing_target_count() {
    let conn = schema();
    let source_id = source(&conn, "legacy-writer");
    let old_file = file(&conn, source_id, "src/old.rs");
    let target_file = file(&conn, source_id, "src/target.rs");
    let moving = chunk(&conn, source_id, old_file);
    let resident = chunk(&conn, source_id, target_file);
    embed(&conn, moving, "model-a", 1);
    embed(&conn, resident, "model-a", 2);

    conn.execute(
        "UPDATE OR REPLACE knowledge_chunk_meta SET file_id = ?1 WHERE chunk_rowid = ?2",
        params![target_file, moving],
    )
    .unwrap();

    assert_eq!(count(&conn, old_file, "model-a"), 0);
    assert_eq!(count(&conn, target_file, "model-a"), 2);
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn conflicting_embedding_primary_key_updates_are_rejected() {
    for recursive_triggers in [false, true] {
        let conn = schema();
        let source_id = source(&conn, "writer");
        let file_id = file(&conn, source_id, "src/lib.rs");
        let first = chunk(&conn, source_id, file_id);
        let second = chunk(&conn, source_id, file_id);
        embed(&conn, first, "model-a", 1);
        embed(&conn, second, "model-a", 2);
        conn.execute_batch(if recursive_triggers {
            "PRAGMA recursive_triggers = ON"
        } else {
            "PRAGMA recursive_triggers = OFF"
        })
        .unwrap();

        let result = conn.execute(
            "UPDATE OR REPLACE knowledge_embeddings SET chunk_rowid = ?1 WHERE chunk_rowid = ?2",
            params![second, first],
        );
        assert!(result.is_err());
        assert_eq!(count(&conn, file_id, "model-a"), 2);
        assert_coverage_matches_canonical(&conn);
    }
}

#[test]
fn conflicting_metadata_primary_key_updates_are_rejected() {
    for recursive_triggers in [false, true] {
        let conn = schema();
        let source_id = source(&conn, "writer");
        let first_file = file(&conn, source_id, "src/first.rs");
        let second_file = file(&conn, source_id, "src/second.rs");
        let first = chunk(&conn, source_id, first_file);
        let second = chunk(&conn, source_id, second_file);
        embed(&conn, first, "model-a", 1);
        embed(&conn, second, "model-a", 2);
        conn.execute_batch(if recursive_triggers {
            "PRAGMA recursive_triggers = ON"
        } else {
            "PRAGMA recursive_triggers = OFF"
        })
        .unwrap();

        let result = conn.execute(
            "UPDATE OR REPLACE knowledge_chunk_meta SET chunk_rowid = ?1 WHERE chunk_rowid = ?2",
            params![second, first],
        );
        assert!(result.is_err());
        assert_eq!(count(&conn, first_file, "model-a"), 1);
        assert_eq!(count(&conn, second_file, "model-a"), 1);
        assert_coverage_matches_canonical(&conn);
    }
}
