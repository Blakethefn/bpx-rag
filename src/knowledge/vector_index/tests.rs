use super::*;

const TEST_DIMENSIONS: usize = 32;
const TEST_MODEL: &str = "test/model-space-v1";

fn vector_for(index: usize) -> Vec<f32> {
    let mut state = (index as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut vector = (0..TEST_DIMENSIONS)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 40) as f32 / ((1_u32 << 24) as f32)) - 0.5
        })
        .collect::<Vec<_>>();
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    vector.iter_mut().for_each(|value| *value /= norm);
    vector
}

fn sample_database(count: usize) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE knowledge_embeddings (
            chunk_rowid INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            dim INTEGER NOT NULL,
            model_id TEXT NOT NULL
         );",
    )
    .unwrap();
    for index in 0..count {
        conn.execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                index as i64 + 1,
                embedder::embedding_to_bytes(&vector_for(index)),
                TEST_DIMENSIONS as i64,
                TEST_MODEL
            ],
        )
        .unwrap();
    }
    conn
}

#[test]
fn hnsw_candidate_search_recalls_exact_neighbors_and_roundtrips_sidecar() {
    let count = 4096;
    let conn = sample_database(count);
    let index = VectorIndex {
        index: build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap(),
        dimensions: TEST_DIMENSIONS,
    };
    let query = vector_for(17);
    let candidates = index.candidates(&query, 10, None).unwrap().unwrap();
    let exact = (0..count)
        .map(|idx| (idx, vector_for(idx)))
        .map(|(idx, vector)| {
            let score = query
                .iter()
                .zip(vector.iter())
                .map(|(a, b)| a * b)
                .sum::<f32>();
            (idx as i64 + 1, score)
        })
        .collect::<Vec<_>>();
    let mut exact = exact;
    exact.sort_by(|left, right| right.1.total_cmp(&left.1));
    let exact_top = exact
        .iter()
        .take(10)
        .map(|(rowid, _)| *rowid)
        .collect::<HashSet<_>>();
    let candidate_set = candidates.iter().copied().collect::<HashSet<_>>();
    let overlap = exact_top.intersection(&candidate_set).count();
    assert!(overlap >= 9, "ANN candidate recall was {overlap}/10");

    let dir = tempfile::tempdir().unwrap();
    save_sidecar(
        dir.path(),
        &index.index,
        TEST_MODEL,
        TEST_DIMENSIONS,
        7,
        "test-database-instance",
        0,
    )
    .unwrap();
    let (restored, manifest) = load_sidecar(
        dir.path(),
        TEST_MODEL,
        TEST_DIMENSIONS,
        7,
        "test-database-instance",
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.size(), count);
    assert_eq!(restored.dimensions(), TEST_DIMENSIONS);
    assert_eq!(manifest.tombstones, 0);
}

#[test]
fn filtered_hnsw_underfill_falls_back_without_expanding_large_allowlist() {
    let count = 4096;
    let conn = sample_database(count);
    let index = VectorIndex {
        index: build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap(),
        dimensions: TEST_DIMENSIONS,
    };
    let allowed = (10_000..10_513).collect::<HashSet<_>>();

    let candidates = index
        .candidates(&vector_for(0), 2000, Some(&allowed))
        .unwrap();

    assert!(candidates.is_none());
}

#[test]
fn sidecar_manifest_rejects_stale_generation_and_database_identity() {
    let count = 8;
    let conn = sample_database(count);
    let index = build_index(&conn, TEST_MODEL, TEST_DIMENSIONS, count).unwrap();
    let dir = tempfile::tempdir().unwrap();
    save_sidecar(
        dir.path(),
        &index,
        TEST_MODEL,
        TEST_DIMENSIONS,
        3,
        "database-a",
        0,
    )
    .unwrap();

    let (_, manifest) = load_sidecar(dir.path(), TEST_MODEL, TEST_DIMENSIONS, 4, "database-a")
        .unwrap()
        .unwrap();
    assert_eq!(manifest.generation, 3);
    assert!(
        load_sidecar(dir.path(), TEST_MODEL, TEST_DIMENSIONS, 3, "database-b")
            .unwrap()
            .is_none()
    );
}

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
        params![embedder::embedding_to_bytes(&replacement)],
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
