use super::super::*;
use super::{sync_dir, write_aged};
use crate::knowledge::open_in_memory;
use crate::knowledge::query::{knowledge_search_report, SearchWeights};
use tempfile::tempdir;

/// Benchmark: time the freshness check for the results of real searches. Run with
/// `cargo test --release freshness_check_latency -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn freshness_check_latency_at_10_50_and_200_results() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    for index in 0..400 {
        write_aged(
            dir.path(),
            &format!("file{index:03}.md"),
            &format!(
                "# File {index}\nshared phrase for result {index}, with enough text to chunk.\n"
            ),
        );
    }
    sync_dir(&store, dir.path());
    for limit in [10u32, 50, 200] {
        let report = knowledge_search_report(
            &store,
            "shared phrase",
            None,
            None,
            limit,
            &SearchWeights::default(),
            200,
        )
        .unwrap();
        let mut samples = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            let states = check_results(&store, &report.results).unwrap();
            samples.push(start.elapsed());
            assert_eq!(states.len(), report.results.len());
        }
        samples.sort();
        println!(
            "freshness_check results={} median_us={} p95_us={}",
            report.results.len(),
            samples[samples.len() / 2].as_micros(),
            samples[samples.len() * 95 / 100].as_micros()
        );
    }
}
