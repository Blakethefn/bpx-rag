use std::collections::HashSet;

use super::super::*;
use super::{sample_database, vector_for, TEST_DIMENSIONS, TEST_MODEL};

#[test]
fn saved_sidecar_applies_incremental_embedding_updates_and_deletes() {
    let count = 5000;
    let conn = sample_database(count);
    conn.execute_batch(
        "CREATE TABLE knowledge_embedding_state (
            state_id INTEGER PRIMARY KEY,
            generation INTEGER NOT NULL,
            database_id TEXT NOT NULL
         );
         INSERT INTO knowledge_embedding_state VALUES (1, 0, 'database-incremental');
         CREATE TABLE knowledge_embedding_changes (
            change_id INTEGER PRIMARY KEY AUTOINCREMENT,
            chunk_rowid INTEGER NOT NULL,
            model_id TEXT NOT NULL,
            operation TEXT NOT NULL
         );
         CREATE TRIGGER changed_embedding_update AFTER UPDATE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
         CREATE TRIGGER changed_embedding_delete AFTER DELETE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (OLD.chunk_rowid, OLD.model_id, 'delete');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;",
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let index = VectorIndex::load_or_build(
        &conn,
        Some(dir.path()),
        TEST_MODEL,
        TEST_DIMENSIONS,
        0,
        "database-incremental",
    )
    .unwrap()
    .unwrap();
    assert_eq!(index.index.size(), count);

    let replacement = vector_for(count + 10);
    conn.execute(
        "UPDATE knowledge_embeddings SET embedding = ?1 WHERE chunk_rowid = 1",
        params![embed::embedding_to_bytes(&replacement)],
    )
    .unwrap();
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let index = VectorIndex::load_or_build(
        &conn,
        Some(dir.path()),
        TEST_MODEL,
        TEST_DIMENSIONS,
        generation,
        "database-incremental",
    )
    .unwrap()
    .unwrap();
    assert!(index
        .candidates(&replacement, 10, None)
        .unwrap()
        .unwrap()
        .contains(&1));

    conn.execute("DELETE FROM knowledge_embeddings WHERE chunk_rowid = 2", [])
        .unwrap();
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let index = VectorIndex::load_or_build(
        &conn,
        Some(dir.path()),
        TEST_MODEL,
        TEST_DIMENSIONS,
        generation,
        "database-incremental",
    )
    .unwrap()
    .unwrap();
    assert_eq!(index.index.size(), count - 1);
    assert!(!index.index.contains(2));
}

fn built_index(count: usize) -> VectorIndex {
    let conn = sample_database(count);
    VectorIndex {
        index: build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap(),
        dimensions: TEST_DIMENSIONS,
    }
}

#[test]
fn small_allowlist_is_returned_whole_even_above_the_limit() {
    let index = built_index(4096);
    let allowed = [7_i64, 42, 4000].into_iter().collect::<HashSet<_>>();

    let mut candidates = index
        .candidates(&vector_for(0), 2, Some(&allowed))
        .unwrap()
        .unwrap();
    candidates.sort_unstable();

    assert_eq!(candidates, vec![7, 42, 4000]);
}

#[test]
fn large_allowlist_with_enough_matches_returns_only_allowed_candidates() {
    let index = built_index(4096);
    let allowed = (1..=600_i64).collect::<HashSet<_>>();

    let candidates = index
        .candidates(&vector_for(0), 10, Some(&allowed))
        .unwrap()
        .unwrap();

    assert!(
        candidates.len() >= 10,
        "expected at least 10 candidates, got {}",
        candidates.len()
    );
    assert!(candidates.iter().all(|rowid| allowed.contains(rowid)));
}
