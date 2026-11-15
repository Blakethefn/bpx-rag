use std::sync::Arc;
use std::time::Instant;

use crate::embed::cache::QueryEmbeddingCache;

use super::super::super::*;
use super::super::latency::{
    load_jina, percentile, store_with_corpus, unit_vector, MODEL_ID, QUERIES,
};

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .expect("read /proc/self/status")
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
        .expect("VmRSS in /proc/self/status")
}

#[test]
#[ignore]
fn cache_memory_at_capacity() {
    let capacity: usize = std::env::var("BPX_BENCH_CACHE_CAPACITY")
        .expect("set BPX_BENCH_CACHE_CAPACITY")
        .parse()
        .expect("BPX_BENCH_CACHE_CAPACITY must be an integer");
    let before = rss_kib();
    let cache = QueryEmbeddingCache::new(capacity);
    for index in 0..capacity {
        cache.insert(
            MODEL_ID,
            &format!("query text number {index:030}"),
            unit_vector(index as u64),
        );
    }
    let after = rss_kib();
    let per_entry = DIM_BYTES + MODEL_ID.len() + 40 + 2 * std::mem::size_of::<String>() + 32;
    println!(
        "RESULT cache_memory capacity={capacity} entries={} rss_delta_kib={} analytic_bytes={}",
        cache.stats().entries,
        after.saturating_sub(before),
        capacity * per_entry
    );
}

const DIM_BYTES: usize = 768 * std::mem::size_of::<f32>();

#[test]
#[ignore]
fn first_pass_with_cache_disabled_at_4k() {
    let embedder = load_jina();
    let mut samples = Vec::new();
    for _ in 0..5 {
        let mut store = store_with_corpus(4_000, Arc::clone(&embedder));
        store.set_query_embedding_cache_capacity(0);
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
    println!(
        "RESULT search_first_pass_cache_disabled chunks=4000 samples={} median_ms={:.3} p95_ms={:.3}",
        samples.len(),
        percentile(&samples, 0.5),
        percentile(&samples, 0.95)
    );
}
