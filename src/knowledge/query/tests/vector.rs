use super::super::vector::knowledge_vector_search;
use super::{make_store, seed_chunk};
use rusqlite::params;

#[test]
fn vector_search_errors_on_non_integer_line_number_instead_of_zero() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lines.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, 2, 'test-model')",
            params![rowid, crate::embed::embedding_to_bytes(&[1.0, 0.0])],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET line_end = 'bad' WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();

    let result = knowledge_vector_search(store.conn(), &[1.0, 0.0], 5, "", &[], "test-model", None);

    assert!(result.is_err(), "line numbers must not default to 0");
}

#[test]
fn vector_search_rejects_embedding_with_wrong_byte_length() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/short.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, 2, 'test-model')",
            params![rowid, vec![0u8, 0u8, 128u8]],
        )
        .unwrap();

    let result = knowledge_vector_search(store.conn(), &[1.0, 0.0], 5, "", &[], "test-model", None);

    assert!(
        result.is_err(),
        "a truncated embedding must not score silently"
    );
}

#[test]
fn vector_search_rejects_non_finite_stored_embedding_instead_of_panicking() {
    let store = make_store();
    for index in 0..60 {
        let rowid = seed_chunk(
            &store,
            "repo",
            "/repo",
            &format!("src/nan{index}.md"),
            &format!("Nan {index}"),
            "whale migration notes",
            "{}",
        );
        let first = if index % 3 == 0 {
            f32::NAN
        } else {
            index as f32 / 60.0
        };
        let bytes: Vec<u8> = [first, 0.5f32]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, 2, 'test-model')",
                params![rowid, bytes],
            )
            .unwrap();
    }

    let result =
        knowledge_vector_search(store.conn(), &[1.0, 0.0], 60, "", &[], "test-model", None);

    assert!(
        result.is_err(),
        "a non-finite stored embedding must fail the query, not reach the score sort"
    );
}
