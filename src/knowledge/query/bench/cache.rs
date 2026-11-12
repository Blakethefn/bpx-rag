use super::super::*;
use super::latency::{load_jina, percentile, store_with_corpus, unit_vector, MODEL_ID, QUERIES};
use crate::embed::cache::QueryEmbeddingCache;
use crate::embed::Embed;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct CountingEmbedder {
    inner: Arc<dyn Embed>,
    calls: Arc<AtomicUsize>,
}

impl Embed for CountingEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_one(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }

    fn dim(&self) -> usize {
        self.inner.dim()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

fn cyclic_workload() -> Vec<&'static str> {
    (0..10).flat_map(|_| QUERIES).collect()
}

fn hot_set_workload() -> Vec<&'static str> {
    (0..400)
        .map(|request| {
            if request % 5 != 0 {
                QUERIES[request % 8]
            } else {
                QUERIES[8 + (request / 5) % 32]
            }
        })
        .collect()
}

struct WorkloadRun {
    hits: u64,
    misses: u64,
    embed_calls: usize,
    samples: Vec<f64>,
}

fn run_workload(requests: &[&str], capacity: usize, embedder: Arc<dyn Embed>) -> WorkloadRun {
    let calls = Arc::new(AtomicUsize::new(0));
    let counting: Arc<dyn Embed> = Arc::new(CountingEmbedder {
        inner: embedder,
        calls: Arc::clone(&calls),
    });
    let mut store = store_with_corpus(4_000, counting);
    store.set_query_embedding_cache_capacity(capacity);
    let samples: Vec<f64> = requests
        .iter()
        .map(|query| {
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
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    let stats = store.query_embedding_cache().stats();
    WorkloadRun {
        hits: stats.hits,
        misses: stats.misses,
        embed_calls: calls.load(Ordering::SeqCst),
        samples,
    }
}

#[test]
#[ignore]
fn repeated_query_latency_by_capacity() {
    let embedder = load_jina();
    let workloads: [(&str, Vec<&str>); 2] = [
        ("cyclic", cyclic_workload()),
        ("hot_set", hot_set_workload()),
    ];
    for (name, requests) in &workloads {
        for capacity in [0_usize, 16, 256] {
            for repeat in 0..3 {
                let run = run_workload(requests, capacity, Arc::clone(&embedder));
                println!(
                    "RESULT repeated workload={name} capacity={capacity} repeat={repeat} \
                     requests={} hits={} misses={} embed_calls={} median_ms={:.3} p95_ms={:.3}",
                    requests.len(),
                    run.hits,
                    run.misses,
                    run.embed_calls,
                    percentile(&run.samples, 0.5),
                    percentile(&run.samples, 0.95)
                );
            }
        }
    }
}

fn per_operation_micros(ops: usize, mut batch: impl FnMut(usize)) -> f64 {
    let batch_ms: Vec<f64> = (0..200)
        .map(|round| {
            let start = Instant::now();
            for op in 0..ops {
                batch(round * ops + op);
            }
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    percentile(&batch_ms, 0.5) * 1_000.0 / ops as f64
}

#[test]
#[ignore]
fn cache_operation_overhead() {
    let vector = unit_vector(3);
    let full = QueryEmbeddingCache::new(256);
    let keys: Vec<String> = (0..256).map(|index| format!("query {index}")).collect();
    for key in &keys {
        full.insert(MODEL_ID, key, vector.clone());
    }
    let disabled = QueryEmbeddingCache::new(0);

    let hit = per_operation_micros(100, |op| {
        full.get(MODEL_ID, &keys[op % 256]).expect("hit");
    });
    let disabled_get = per_operation_micros(100, |op| {
        assert!(disabled.get(MODEL_ID, &keys[op % 256]).is_none());
    });
    let insert_existing = per_operation_micros(100, |op| {
        full.insert(MODEL_ID, &keys[op % 256], vector.clone());
    });
    let insert_with_eviction = per_operation_micros(100, |op| {
        full.insert(MODEL_ID, &format!("fresh {op}"), vector.clone());
    });
    println!(
        "RESULT overhead hit_get_us={hit:.3} disabled_get_us={disabled_get:.3} \
         insert_existing_us={insert_existing:.3} insert_with_eviction_us={insert_with_eviction:.3}"
    );
}

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
