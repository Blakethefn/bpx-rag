use super::*;

#[test]
fn incremental_replay_reserves_for_new_rows_and_replacements() {
    let initial = 8_192;
    let previous_deletes = 1_024;
    let new_rows = 1_536;
    let replacements = 512;
    let expected = initial - previous_deletes + new_rows;
    let conn = sample_database(initial);
    conn.execute_batch(
        "CREATE TABLE knowledge_embedding_state (
            state_id INTEGER PRIMARY KEY,
            generation INTEGER NOT NULL,
            database_id TEXT NOT NULL
         );
         INSERT INTO knowledge_embedding_state VALUES (1, 0, 'database-reserve');
         CREATE TABLE knowledge_embedding_changes (
            change_id INTEGER PRIMARY KEY AUTOINCREMENT,
            chunk_rowid INTEGER NOT NULL,
            model_id TEXT NOT NULL,
            operation TEXT NOT NULL
         );
         CREATE TRIGGER changed_embedding_insert AFTER INSERT ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
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
        "database-reserve",
    )
    .unwrap()
    .unwrap();
    assert_eq!(index.index.size(), initial);
    assert_eq!(index.index.capacity(), initial);
    drop(index);

    let (index, _) = load_sidecar(
        dir.path(),
        TEST_MODEL,
        TEST_DIMENSIONS,
        0,
        "database-reserve",
    )
    .unwrap()
    .unwrap();
    let mut tombstones = 0;
    for rowid in 1..=previous_deletes {
        conn.execute(
            "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
            [rowid as i64],
        )
        .unwrap();
        tombstones += index.remove(rowid as u64).unwrap();
    }
    let deleted_generation: i64 = conn
        .query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    save_sidecar(
        dir.path(),
        &index,
        TEST_MODEL,
        TEST_DIMENSIONS,
        deleted_generation,
        "database-reserve",
        tombstones,
    )
    .unwrap();

    for offset in 1..=new_rows {
        let rowid = initial + offset;
        conn.execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                rowid as i64,
                embed::embedding_to_bytes(&vector_for(rowid)),
                TEST_DIMENSIONS as i64,
                TEST_MODEL
            ],
        )
        .unwrap();
    }
    for rowid in previous_deletes + 1..=previous_deletes + replacements {
        conn.execute(
            "UPDATE knowledge_embeddings SET embedding = ?1 WHERE chunk_rowid = ?2",
            params![
                embed::embedding_to_bytes(&vector_for(expected + rowid)),
                rowid as i64
            ],
        )
        .unwrap();
    }
    let generation: i64 = conn
        .query_row(
            "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let updated = VectorIndex::load_or_build(
        &conn,
        Some(dir.path()),
        TEST_MODEL,
        TEST_DIMENSIONS,
        generation,
        "database-reserve",
    )
    .unwrap()
    .unwrap();

    assert_eq!(updated.index.size(), expected);
    assert!(
        updated.index.capacity() >= expected + new_rows + replacements,
        "capacity must include replay headroom for tombstones and additions"
    );
    let replacement = vector_for(expected + previous_deletes + 1);
    let nearest = updated.candidates(&replacement, 1, None).unwrap().unwrap();
    assert_eq!(nearest.first(), Some(&(previous_deletes as i64 + 1)));
}
