use std::collections::HashSet;

use super::*;
mod sidecar;
mod manifest;

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
                embed::embedding_to_bytes(&vector_for(index)),
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

