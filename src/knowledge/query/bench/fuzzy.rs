use std::time::{Duration, Instant};

use super::super::fuzzy::knowledge_fuzzy_search;

mod corpus;
mod timing;

use corpus::{build_store, misspelling_queries, synthetic_vocabulary};
use timing::{
    full_vocabulary_read, mean_levenshtein_calls, ms, percentile, resident_kb, time_queries,
};

const VOCABULARY_SIZES: [usize; 3] = [10_000, 100_000, 500_000];

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

        let cold_start = Instant::now();
        knowledge_fuzzy_search(&store, &single[0], 10, "", &[]).unwrap();
        let cold_first_query = cold_start.elapsed();
        let rss_after_cold = resident_kb();

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
