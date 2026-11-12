//! Latency and memory benchmark for fuzzy correction. Ignored by default.
//!
//! Run with:
//! `cargo test --bin bpx-rag fuzzy_latency_benchmark -- --ignored --nocapture`
//!
//! The vocabulary comes from a fixed-seed generator and the misspelling set is
//! derived from it, so every run measures the same workload.

use super::super::fuzzy::{fuzzy_correct, knowledge_fuzzy_search};
use super::super::*;
use crate::knowledge;
use rusqlite::params;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const VOCABULARY_SIZES: [usize; 3] = [10_000, 100_000, 500_000];
const WORDS_PER_CHUNK: usize = 50;
const QUERY_COUNT: usize = 200;
const MIN_WORD_LEN: usize = 4;
const MAX_WORD_LEN: usize = 12;
const TIMED_RUNS_FULL_SCAN: usize = 20;
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// xorshift64. A fixed seed keeps the vocabulary identical across runs.
struct Xorshift(u64);

impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// `size` distinct lowercase words with lengths from 4 to 12.
fn synthetic_vocabulary(size: usize) -> Vec<String> {
    let mut rng = Xorshift(SEED);
    let mut seen = HashSet::with_capacity(size);
    let mut words = Vec::with_capacity(size);
    while words.len() < size {
        let len = MIN_WORD_LEN + rng.below(MAX_WORD_LEN - MIN_WORD_LEN + 1);
        let word: String = (0..len)
            .map(|_| char::from(b'a' + rng.below(26) as u8))
            .collect();
        if seen.insert(word.clone()) {
            words.push(word);
        }
    }
    words
}

/// One edit away from `word`. The edit kind cycles: delete, substitute, insert.
fn typo(word: &str, kind: usize) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    match kind % 3 {
        0 => {
            chars.remove(chars.len() / 2);
        }
        1 => {
            let middle = chars.len() / 2;
            chars[middle] = if chars[middle] == 'x' { 'y' } else { 'x' };
        }
        _ => chars.push('k'),
    }
    chars.into_iter().collect()
}

/// Single-word typos spread across the vocabulary, plus three-word queries with
/// one typo and two correct words.
fn misspelling_queries(vocabulary: &[String]) -> (Vec<String>, Vec<String>) {
    let single = (0..QUERY_COUNT)
        .map(|k| typo(&vocabulary[k * vocabulary.len() / QUERY_COUNT], k))
        .collect();
    let multi = (0..QUERY_COUNT)
        .map(|k| {
            let typo_word = typo(&vocabulary[k * vocabulary.len() / QUERY_COUNT], k);
            let first = &vocabulary[(k * 7919 + 1) % vocabulary.len()];
            let second = &vocabulary[(k * 104_729 + 2) % vocabulary.len()];
            format!("{typo_word} {first} {second}")
        })
        .collect();
    (single, multi)
}

/// Insert the vocabulary as chunks so that the content vocab holds every word.
fn build_store(vocabulary: &[String]) -> KnowledgeStore {
    let store = knowledge::open_in_memory().expect("in-memory knowledge store");
    let conn = store.conn();
    conn.execute_batch("BEGIN").unwrap();
    for chunk in vocabulary.chunks(WORDS_PER_CHUNK) {
        conn.execute(
            "INSERT INTO knowledge_chunks (title, content) VALUES ('', ?1)",
            params![chunk.join(" ")],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    store
}

/// Resident set size in kilobytes from `/proc/self/status` (Linux only).
fn resident_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}

fn time_queries(store: &KnowledgeStore, queries: &[String]) -> Vec<Duration> {
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

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let rank = ((fraction * sorted.len() as f64).ceil() as usize).max(1);
    sorted[rank - 1]
}

/// Mean Levenshtein computations per query. Counted outside the timed loop.
fn mean_levenshtein_calls(store: &KnowledgeStore, queries: &[String]) -> f64 {
    let total: usize = queries
        .iter()
        .map(|query| fuzzy_correct(store, query).unwrap().levenshtein_calls)
        .sum();
    total as f64 / queries.len() as f64
}

/// Median time of the full content-vocabulary read used by the fuzzy layer.
fn full_vocabulary_read(store: &KnowledgeStore) -> (Duration, usize) {
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

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture"]
fn fuzzy_latency_benchmark() {
    for size in VOCABULARY_SIZES {
        let vocabulary = synthetic_vocabulary(size);
        let (single, multi) = misspelling_queries(&vocabulary);

        let rss_before = resident_kb();
        let store = build_store(&vocabulary);
        let rss_after_build = resident_kb();
        let (full_read, indexed_terms) = full_vocabulary_read(&store);

        // The first query pays for the vocabulary load. Time it on its own.
        let cold_start = Instant::now();
        knowledge_fuzzy_search(&store, &single[0], 10, "", &[]).unwrap();
        let cold_first_query = cold_start.elapsed();
        let rss_after_cold = resident_kb();

        // Warm the page cache and the statement cache before timing.
        time_queries(&store, &single);
        time_queries(&store, &multi);

        let single_timings = time_queries(&store, &single);
        let multi_timings = time_queries(&store, &multi);
        let mut all: Vec<Duration> = single_timings
            .iter()
            .chain(multi_timings.iter())
            .copied()
            .collect();
        all.sort();

        let single_calls = mean_levenshtein_calls(&store, &single);
        let multi_calls = mean_levenshtein_calls(&store, &multi);

        println!(
            "vocab_requested={size} vocab_indexed={indexed_terms} queries={} \
             p50_ms={:.3} p95_ms={:.3} single_p95_ms={:.3} multi_p95_ms={:.3} \
             cold_first_query_ms={:.3} full_vocab_read_ms={:.3} \
             lev_calls_single={single_calls:.1} lev_calls_multi={multi_calls:.1} \
             rss_build_delta_kb={} rss_cache_delta_kb={} rss_after_build_kb={}",
            all.len(),
            ms(percentile(&all, 0.50)),
            ms(percentile(&all, 0.95)),
            ms(percentile(&single_timings, 0.95)),
            ms(percentile(&multi_timings, 0.95)),
            ms(cold_first_query),
            ms(full_read),
            rss_after_build.saturating_sub(rss_before),
            rss_after_cold.saturating_sub(rss_after_build),
            rss_after_build,
        );
    }
}
