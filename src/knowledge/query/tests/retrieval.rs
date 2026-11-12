use super::super::*;
use super::super::vector::knowledge_vector_search;
use super::{make_store, seed_chunk};
use crate::embed::Embed;
use rusqlite::params;
use std::sync::Arc;

struct SemanticTestEmbedder;

impl Embed for SemanticTestEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(
            if text == "whale" || text.contains("blue ocean migration") {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            },
        )
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }

    fn dim(&self) -> usize {
        2
    }
}

#[test]
fn lexical_search_matches_individual_terms_in_natural_language_queries() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/search.rs",
        "Hybrid retrieval layers",
        "The pipeline runs BM25, trigram, fuzzy, and semantic vector retrieval, then merges ranked results.",
        "{}",
    );

    let results = knowledge_search(
        &store,
        "Which code runs BM25 and semantic vector retrieval across the corpus?",
        None,
        Some("repo"),
        10,
        &SearchWeights {
            keyword_weight: 1.0,
            vector_weight: 0.0,
        },
        2000,
    )
    .unwrap();

    assert_eq!(results[0].file_path, "src/search.rs");
    assert!(results[0].why.layers.bm25 > 0.0);
}

#[test]
fn dense_retrieval_can_find_a_chunk_with_no_lexical_hit() {
    let mut store = make_store();
    store.set_embedder(Arc::new(SemanticTestEmbedder));
    let lexical = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lexical.md",
        "Lexical distractor",
        "whale appears in a glossary but does not describe movement",
        "{}",
    );
    let semantic = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/migration.md",
        "Ocean migration",
        "blue ocean migration paths of large mammals",
        "{}",
    );
    for (rowid, content) in [
        (
            lexical,
            "whale appears in a glossary but does not describe movement",
        ),
        (semantic, "blue ocean migration paths of large mammals"),
    ] {
        let embedding = SemanticTestEmbedder.embed_one(content).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim) VALUES (?1, ?2, 2)",
                params![rowid, embed::embedding_to_bytes(&embedding)],
            )
            .unwrap();
    }

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

    assert!(results.iter().any(|result| {
        result.file_path == "src/migration.md" && result.why.layers.vector > 0.0
    }));
}

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
        "",
        &[],
        "sentence-transformers/all-MiniLM-L6-v2",
        Some(&[]),
    )
    .unwrap();

    assert!(results.is_empty());
}
