use std::time::{Duration, Instant};

use super::super::super::fuzzy::{fuzzy_correct, knowledge_fuzzy_search};
use super::super::super::KnowledgeStore;

const TIMED_RUNS_FULL_SCAN: usize = 20;

pub(super) fn resident_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}

pub(super) fn time_queries(store: &KnowledgeStore, queries: &[String]) -> Vec<Duration> {
    let mut timings: Vec<Duration> = queries
        .iter()
        .map(|query| {
            let start = Instant::now();
            knowledge_fuzzy_search(store, query, 10, "", &[]).unwrap();
            start.elapsed()
        })
        .collect();
    timings.sort();
    timings
}

pub(super) fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let rank = ((fraction * sorted.len() as f64).ceil() as usize).max(1);
    sorted[rank - 1]
}

pub(super) fn mean_levenshtein_calls(store: &KnowledgeStore, queries: &[String]) -> f64 {
    let total: usize = queries
        .iter()
        .map(|query| fuzzy_correct(store, query).unwrap().levenshtein_calls)
        .sum();
    total as f64 / queries.len() as f64
}

pub(super) fn full_vocabulary_read(store: &KnowledgeStore) -> (Duration, usize) {
    let conn = store.conn();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab \
         USING fts5vocab(knowledge_chunks, instance);",
    )
    .unwrap();
    let mut timings = Vec::with_capacity(TIMED_RUNS_FULL_SCAN);
    let mut term_count = 0;
    for _ in 0..TIMED_RUNS_FULL_SCAN {
        let start = Instant::now();
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT term FROM knowledge_chunks_vocab WHERE col = 'content' ORDER BY term",
            )
            .unwrap();
        term_count = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .count();
        timings.push(start.elapsed());
    }
    timings.sort();
    (timings[timings.len() / 2], term_count)
}

pub(super) fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
