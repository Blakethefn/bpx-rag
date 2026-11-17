use std::sync::Arc;

use rusqlite::params;

use crate::embed::{self, Embed};

use super::super::super::vector::knowledge_vector_search;
use super::super::super::*;
use super::super::{make_store, seed_chunk};
use super::SemanticTestEmbedder;

#[test]
fn dense_retrieval_ignores_vectors_from_a_different_model() {
    let mut store = make_store();
    store.set_embedder(Arc::new(SemanticTestEmbedder));
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/migration.md",
        "Ocean migration",
        "blue ocean migration paths of large mammals",
        "{}",
    );
    let incompatible = SemanticTestEmbedder
        .embed_one("blue ocean migration")
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, ?2, 2, 'jinaai/jina-embeddings-v2-base-code')",
            params![rowid, embed::embedding_to_bytes(&incompatible)],
        )
        .unwrap();

    let results = knowledge_search(
        &store,
        "whale",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert!(
        results.is_empty(),
        "vectors from a foreign model must not be compared"
    );
}

#[test]
fn explicit_empty_vector_candidates_return_no_results() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/vector.md",
        "Vector chunk",
        "text that would otherwise be searchable",
        "{}",
    );
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
             VALUES (?1, ?2, 2)",
            params![rowid, embed::embedding_to_bytes(&[1.0, 0.0])],
        )
        .unwrap();

    let results = knowledge_vector_search(
        store.conn(),
        &[1.0, 0.0],
        10,
        &[],
        "sentence-transformers/all-MiniLM-L6-v2",
        Some(&[]),
    )
    .unwrap();

    assert!(results.is_empty());
}
