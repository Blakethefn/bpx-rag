use std::sync::Arc;
use std::time::Instant;

use rusqlite::params;

use crate::embed::{self, Embed};

use super::super::tests::seed_chunk;
use super::super::vector::knowledge_vector_search;
use super::super::*;
use crate::support::make_store;

mod model;

use model::DIM;
pub(super) use model::{load_jina, unit_vector, MODEL_ID, QUERIES};

pub(super) fn synthetic_corpus(store: &KnowledgeStore, chunks: usize) {
    for index in 0..chunks {
        let rel_path = format!("src/chunk_{index}.rs");
        let title = format!("fn chunk_{index}");
        let content = format!("pub fn chunk_{index}(value: usize) -> usize {{ value + {index} }}");
        let rowid = seed_chunk(store, "repo", "/repo", &rel_path, &title, &content, "{}");
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    rowid,
                    embed::embedding_to_bytes(&unit_vector(index as u64)),
                    DIM as i64,
                    MODEL_ID
                ],
            )
            .expect("insert synthetic embedding");
    }
}

pub(super) fn store_with_corpus(chunks: usize, embedder: Arc<dyn Embed>) -> KnowledgeStore {
    let mut store = make_store();
    store.set_embedder(embedder);
    synthetic_corpus(&store, chunks);
    store
}

pub(super) fn time_runs(warmup: usize, runs: usize, mut run: impl FnMut()) -> Vec<f64> {
    for _ in 0..warmup {
        run();
    }
    (0..runs)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect()
}

pub(super) fn percentile(samples: &[f64], fraction: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((sorted.len() as f64 * fraction).ceil() as usize).max(1);
    sorted[rank - 1]
}

fn report(label: &str, chunks: usize, samples: &[f64]) {
    println!(
        "RESULT {label} chunks={chunks} samples={} median_ms={:.3} p95_ms={:.3}",
        samples.len(),
        percentile(samples, 0.5),
        percentile(samples, 0.95)
    );
}

#[test]
#[ignore]
fn h2_exact_vector_decode_scaling() {
    for chunks in [1_000, 4_000, 20_000] {
        let store = make_store();
        synthetic_corpus(&store, chunks);
        let query = unit_vector(7);
        let samples = time_runs(3, 15, || {
            knowledge_vector_search(store.conn(), &query, 10, &[], MODEL_ID, None)
                .expect("exact vector search");
        });
        report("h2_exact_vector_search", chunks, &samples);
    }
}

#[test]
#[ignore]
fn cold_query_embedding_time() {
    let embedder = load_jina();
    for query in QUERIES.iter().take(3) {
        embedder.embed_one(query).expect("warm-up embedding");
    }
    let samples: Vec<f64> = QUERIES
        .iter()
        .map(|query| {
            let start = Instant::now();
            embedder.embed_one(query).expect("query embedding");
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    report("cold_query_embedding", 0, &samples);
}

#[test]
#[ignore]
fn search_latency_first_pass_at_4k() {
    let embedder = load_jina();
    let mut samples = Vec::new();
    for _ in 0..5 {
        let store = store_with_corpus(4_000, Arc::clone(&embedder));
        for query in QUERIES {
            let start = Instant::now();
            knowledge_search_report(
                &store,
                query,
                None,
                None,
                10,
                &SearchWeights::default(),
                1_000,
            )
            .expect("search");
            samples.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
    }
    report("search_first_pass", 4_000, &samples);
}
