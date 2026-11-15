use std::collections::HashSet;

use super::super::*;
use super::{sample_database, vector_for, TEST_DIMENSIONS, TEST_MODEL};

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
