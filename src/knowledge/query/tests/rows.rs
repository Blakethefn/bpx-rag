use super::super::*;
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

fn corrupt_metadata_as_blob(store: &KnowledgeStore, chunk_rowid: i64) {
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET metadata = X'FFFE' WHERE chunk_rowid = ?1",
            params![chunk_rowid],
        )
        .unwrap();
}

mod decode;
