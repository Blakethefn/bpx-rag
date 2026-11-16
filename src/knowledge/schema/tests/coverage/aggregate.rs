use super::*;

mod meta;
mod replace;

#[test]
fn file_source_directory_and_deletion_coverage_matches_canonical_counts() {
    let conn = schema();
    let first_source = source(&conn, "first");
    let first_file = file(&conn, first_source, "src/a.rs");
    let nested_file = file(&conn, first_source, "src/nested/b.rs");
    let outside_file = file(&conn, first_source, "src-old/c.rs");
    let second_source = source(&conn, "second");
    let other_file = file(&conn, second_source, "src/d.rs");
    for (file_id, count) in [
        (first_file, 2),
        (nested_file, 1),
        (outside_file, 2),
        (other_file, 1),
    ] {
        for _ in 0..count {
            let source_id = if file_id == other_file {
                second_source
            } else {
                first_source
            };
            let rowid = chunk(&conn, source_id, file_id);
            embed(&conn, rowid, "model-a", 1);
        }
    }
    assert_coverage_matches_canonical(&conn);

    let per_file: i64 = conn
        .query_row(
            "SELECT embedded_count FROM knowledge_embedding_coverage
             WHERE file_id = ?1 AND model_id = 'model-a'",
            [first_file],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(per_file, 2);
    assert_eq!(
        coverage_sum(&conn, first_source, ""),
        canonical_sum(&conn, first_source, "")
    );
    let directory = "AND f.rel_path >= 'src/' AND f.rel_path < 'src0'";
    assert_eq!(
        coverage_sum(&conn, first_source, directory),
        canonical_sum(&conn, first_source, directory)
    );

    let first_chunk: i64 = conn
        .query_row(
            "SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1 LIMIT 1",
            [first_file],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "DELETE FROM knowledge_chunk_meta WHERE chunk_rowid = ?1",
        [first_chunk],
    )
    .unwrap();
    assert_coverage_matches_canonical(&conn);
    conn.execute("DELETE FROM knowledge_files WHERE id = ?1", [nested_file])
        .unwrap();
    assert_coverage_matches_canonical(&conn);
    conn.execute(
        "DELETE FROM knowledge_sources WHERE id = ?1",
        [second_source],
    )
    .unwrap();
    assert_coverage_matches_canonical(&conn);
}

#[test]
fn missing_trigger_rebuilds_coverage_before_restoring_trigger_tracking() {
    let conn = schema();
    let source_id = source(&conn, "repo");
    let file_id = file(&conn, source_id, "src/lib.rs");
    let first = chunk(&conn, source_id, file_id);
    embed(&conn, first, "model-a", 1);
    conn.execute_batch("DROP TRIGGER knowledge_embeddings_coverage_insert")
        .unwrap();

    let second = chunk(&conn, source_id, file_id);
    embed(&conn, second, "model-a", 2);
    assert_eq!(count(&conn, file_id, "model-a"), 1);

    apply_schema(&conn).unwrap();
    assert_eq!(count(&conn, file_id, "model-a"), 2);
    let third = chunk(&conn, source_id, file_id);
    embed(&conn, third, "model-a", 3);
    assert_eq!(count(&conn, file_id, "model-a"), 3);
    assert_coverage_matches_canonical(&conn);
}
