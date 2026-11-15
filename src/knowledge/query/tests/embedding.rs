use super::super::*;
use super::{make_store, seed_chunk};
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

mod cache;

fn search_fingerprint(store: &KnowledgeStore, query: &str) -> String {
    let report = search(store, query);
    format!(
        "{}\n{:?}\n{:?}",
        serde_json::to_string(&report.results).unwrap(),
        report.layers,
        report.warnings
    )
}
