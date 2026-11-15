use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::embed::Embed;

use super::super::super::*;
use super::super::latency::{load_jina, percentile, store_with_corpus, QUERIES};

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
