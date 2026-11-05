//! Query embedding cache on the search path: reuse, misses, failures, and result parity.

use super::tests::{make_store, seed_chunk};
use super::*;
use crate::embed::{self, Embed};
use rusqlite::params;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";
const OTHER_MODEL: &str = "test/other-model";

const CORPUS: [(&str, &str, &str); 4] = [
    (
        "src/whales.md",
        "Whale migration",
        "ocean migration of humpback whales",
    ),
    ("src/glossary.md", "Glossary", "whale song glossary terms"),
    (
        "src/queue.md",
        "Queue",
        "durable queue acknowledgement semantics",
    ),
    (
        "src/currents.md",
        "Currents",
        "ocean currents circulate heat",
    ),
];

#[derive(Clone)]
enum Output {
    Semantic,
    Failure,
    NonFinite,
}

/// Counts `embed_one` calls, so tests can tell cache hits from real embeddings.
struct CountingEmbedder {
    model: &'static str,
    output: Output,
    calls: Arc<AtomicUsize>,
}

impl Embed for CountingEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.output {
            Output::Semantic => Ok(semantic_vector(text)),
            Output::Failure => Err(anyhow::anyhow!("synthetic query embedding failure")),
            Output::NonFinite => Ok(vec![f32::NAN, 0.0]),
        }
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }

    fn dim(&self) -> usize {
        2
    }

    fn model_id(&self) -> &str {
        self.model
    }
}

fn semantic_vector(text: &str) -> Vec<f32> {
    if text.contains("ocean") || text.contains("whale") {
        vec![1.0, 0.0]
    } else {
        vec![0.0, 1.0]
    }
}

/// A store with the corpus indexed under `model`, and the embedder attached.
fn seeded_store(output: Output, model: &'static str) -> (KnowledgeStore, Arc<AtomicUsize>) {
    let mut store = make_store();
    let calls = Arc::new(AtomicUsize::new(0));
    store.set_embedder(Arc::new(CountingEmbedder {
        model,
        output,
        calls: Arc::clone(&calls),
    }));
    for (rel_path, title, content) in CORPUS {
        let rowid = seed_chunk(&store, "repo", "/repo", rel_path, title, content, "{}");
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, 2, ?3)",
                params![
                    rowid,
                    embed::embedding_to_bytes(&semantic_vector(content)),
                    model
                ],
            )
            .unwrap();
    }
    (store, calls)
}

fn search(store: &KnowledgeStore, query: &str) -> SearchReport {
    knowledge_search_report(
        store,
        query,
        None,
        None,
        10,
        &SearchWeights::default(),
        1_000,
    )
    .unwrap()
}

fn vector_layer_failed(report: &SearchReport) -> bool {
    report.layers.iter().any(|layer| {
        layer.layer == "vector" && matches!(layer.outcome, LayerOutcome::Failed { .. })
    })
}

#[test]
fn repeated_query_embeds_once_and_reuses_the_vector() {
    let (store, calls) = seeded_store(Output::Semantic, MODEL);

    search(&store, "ocean migration");
    search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let stats = store.query_embedding_cache().stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[test]
fn distinct_queries_each_embed_once() {
    let (store, calls) = seeded_store(Output::Semantic, MODEL);

    search(&store, "ocean migration");
    search(&store, "whale song");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn zero_capacity_embeds_on_every_search() {
    let (mut store, calls) = seeded_store(Output::Semantic, MODEL);
    store.set_query_embedding_cache_capacity(0);

    search(&store, "ocean migration");
    search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn changing_the_model_misses_the_cache() {
    let (mut store, calls) = seeded_store(Output::Semantic, MODEL);
    search(&store, "ocean migration");

    store.set_embedder(Arc::new(CountingEmbedder {
        model: OTHER_MODEL,
        output: Output::Semantic,
        calls: Arc::clone(&calls),
    }));
    search(&store, "ocean migration");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the new model must embed the query itself"
    );
}

#[test]
fn failed_query_embedding_is_not_cached() {
    let (store, calls) = seeded_store(Output::Failure, MODEL);

    let first = search(&store, "ocean migration");
    let second = search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(vector_layer_failed(&first));
    assert!(vector_layer_failed(&second));
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn invalid_query_vector_is_not_cached() {
    let (store, calls) = seeded_store(Output::NonFinite, MODEL);

    let first = search(&store, "ocean migration");
    let second = search(&store, "ocean migration");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(vector_layer_failed(&first));
    assert!(vector_layer_failed(&second));
    assert_eq!(store.query_embedding_cache().stats().entries, 0);
}

#[test]
fn cached_and_uncached_searches_return_identical_results() {
    let (cached, _) = seeded_store(Output::Semantic, MODEL);
    let (mut uncached, _) = seeded_store(Output::Semantic, MODEL);
    uncached.set_query_embedding_cache_capacity(0);

    for query in ["ocean migration", "whale song", "queue acknowledgement"] {
        let expected = search_fingerprint(&uncached, query);
        assert_eq!(
            search_fingerprint(&cached, query),
            expected,
            "miss path: {query}"
        );
        assert_eq!(
            search_fingerprint(&cached, query),
            expected,
            "hit path: {query}"
        );
    }
    assert!(cached.query_embedding_cache().stats().hits >= 3);
}

/// Everything a caller can observe from a search, serialized for comparison.
fn search_fingerprint(store: &KnowledgeStore, query: &str) -> String {
    let report = search(store, query);
    format!(
        "{}\n{:?}\n{:?}",
        serde_json::to_string(&report.results).unwrap(),
        report.layers,
        report.warnings
    )
}
