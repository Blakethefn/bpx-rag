//! Stage 8 latency measurements that compile against both HEAD and the cache branch.
//!
//! Ignored by default. Set `BPX_BENCH_MODEL_DIR` to the jina model directory and run:
//! `cargo test --release --bin bpx-rag bench::latency -- --ignored --nocapture`
//!
//! The corpus is synthetic: deterministic chunk text and random unit vectors. Only time and
//! bytes are measured, not ranking quality.

use super::super::tests::seed_chunk;
use super::super::vector::knowledge_vector_search;
use super::super::*;
use crate::config::EmbeddingDevice;
use crate::embed::{self, Embed, Embedder, CODE_MODEL};
use crate::support::make_store;
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub(super) const MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-code";
const DIM: usize = 768;

/// The fixed query set for every Stage 8 measurement. Do not change it after measuring.
pub(super) const QUERIES: [&str; 40] = [
    "where is the watch debounce window configured",
    "how does sync decide which files changed",
    "retry policy for failed embedding batches",
    "symbol lookup by exact name",
    "freshness state for stale indexed files",
    "escape user input before building an FTS query",
    "vector similarity threshold for approximate search",
    "rank fusion constant and layer weights",
    "load the cached ONNX model without network access",
    "diversify results so one file does not dominate",
    "trigram candidate pool when BM25 fills the limit",
    "fuzzy correction over the vocabulary cache",
    "identifier splitting for camelCase names",
    "remove a knowledge source and its chunks",
    "enrichment metadata parse error handling",
    "relation neighbors for the top hit",
    "database schema migration for embeddings",
    "why a result was returned explain output",
    "MCP tool arguments for search limit",
    "count only summary token estimate",
    "transaction rollback after a failed write",
    "writer lock for the knowledge database",
    "stale embedding generation after model change",
    "chunk size limit in bytes",
    "source glob patterns and scope encoding",
    "lexical search over file paths",
    "graph rerank link boost",
    "CUDA device selection for embeddings",
    "progress output during model download",
    "batch size for embed_batch calls",
    "normalize embedding vectors to unit length",
    "approximate index rebuild when rows change",
    "embedding blob decode length check",
    "watch status and control commands",
    "fresh versus stale result labels",
    "symbol index table and kinds",
    "parse failures in source metadata",
    "query profile effective query text",
    "snippet truncation for lower ranked results",
    "eval harness repetitions and compare script",
];

/// SplitMix64 step. Deterministic, so every run sees the same corpus.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A deterministic unit vector of `DIM` dimensions.
pub(super) fn unit_vector(seed: u64) -> Vec<f32> {
    let mut state = seed;
    let raw: Vec<f32> = (0..DIM)
        .map(|_| {
            let unit = (splitmix(&mut state) >> 11) as f64 / (1_u64 << 53) as f64;
            (unit * 2.0 - 1.0) as f32
        })
        .collect();
    let norm = raw.iter().map(|value| value * value).sum::<f32>().sqrt();
    raw.iter().map(|value| value / norm).collect()
}

/// Index `chunks` synthetic chunks, each with a stored embedding for `MODEL_ID`.
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

/// A store with `chunks` synthetic chunks and `embedder` attached.
pub(super) fn store_with_corpus(chunks: usize, embedder: Arc<dyn Embed>) -> KnowledgeStore {
    let mut store = make_store();
    store.set_embedder(embedder);
    synthetic_corpus(&store, chunks);
    store
}

/// The real jina embedder on CPU. Loads only a complete local cache entry.
pub(super) fn load_jina() -> Arc<dyn Embed> {
    let model_dir = std::env::var_os("BPX_BENCH_MODEL_DIR")
        .expect("set BPX_BENCH_MODEL_DIR to the jina model directory");
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let embedder = Embedder::load_cached_with_device(
        Path::new(&model_dir),
        CODE_MODEL,
        16,
        threads,
        EmbeddingDevice::Cpu,
    )
    .expect("load cached jina model");
    Arc::new(embedder)
}

/// Milliseconds for `runs` timed calls after `warmup` untimed calls.
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

/// Nearest-rank percentile over unsorted samples.
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
            knowledge_vector_search(store.conn(), &query, 10, "", &[], MODEL_ID, None)
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
        // A fresh store per pass, so every query is a first-time query for this tree.
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
