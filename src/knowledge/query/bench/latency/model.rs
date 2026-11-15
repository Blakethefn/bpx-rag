use std::path::Path;
use std::sync::Arc;

use crate::config::EmbeddingDevice;
use crate::embed::{Embed, Embedder, CODE_MODEL};

pub(in super::super) const MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-code";
pub(super) const DIM: usize = 768;

pub(in super::super) const QUERIES: [&str; 40] = [
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

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub(in super::super) fn unit_vector(seed: u64) -> Vec<f32> {
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

pub(in super::super) fn load_jina() -> Arc<dyn Embed> {
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
