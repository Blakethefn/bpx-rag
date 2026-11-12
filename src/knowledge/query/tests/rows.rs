use super::super::*;
use super::super::lexical::{knowledge_bm25_search, knowledge_trigram_search};
use super::{make_store, seed_chunk};
use crate::embed::Embed;
use rusqlite::params;
use std::sync::Arc;

struct BrokenQueryEmbedder;

impl Embed for BrokenQueryEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        anyhow::bail!("synthetic query embedding failure")
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }

    fn dim(&self) -> usize {
        4
    }

    fn max_batch_size(&self) -> usize {
        16
    }

    fn model_id(&self) -> &str {
        "broken-test-model"
    }
}

#[test]
fn failed_query_embedding_is_reported_and_lexical_results_still_return() {
    let mut store = make_store();
    store.set_embedder(Arc::new(BrokenQueryEmbedder));
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/whale.md",
        "Whale notes",
        "whale migration routes across the ocean",
        "{}",
    );

    let report = knowledge_search_report(
        &store,
        "whale migration",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    )
    .unwrap();

    assert!(!report.results.is_empty(), "lexical layers still answer");
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("vector search was skipped"));
    assert!(report.warnings[0].contains("synthetic query embedding failure"));
}

// ── Malformed database rows ──────────────────────────────────────────────
//
// A row that cannot be decoded is database corruption. Each retrieval layer
// must return an error for it, not drop the row or substitute a default.

/// Overwrite a chunk's metadata with bytes that cannot decode as TEXT.
fn corrupt_metadata_as_blob(store: &KnowledgeStore, chunk_rowid: i64) {
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET metadata = X'FFFE' WHERE chunk_rowid = ?1",
            params![chunk_rowid],
        )
        .unwrap();
}

#[test]
fn bm25_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn trigram_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_trigram_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn bm25_search_errors_on_non_integer_line_number_instead_of_zero() {
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
            "UPDATE knowledge_chunk_meta SET line_start = 'bad' WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "line numbers must not default to 0");
}

#[test]
fn search_errors_on_invalid_chunk_metadata_instead_of_defaulting_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/bad_meta.md",
        "Whale notes",
        "whale migration notes",
        "{not json",
    );

    let result = knowledge_search_report(
        &store,
        "whale migration",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    );

    assert!(result.is_err(), "invalid metadata JSON must be reported");
}
