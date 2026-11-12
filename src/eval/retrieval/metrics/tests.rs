use super::{measure_query, MetricSums, UnanswerableMetricSums};
use crate::eval::{QualityMetrics, UnanswerableMetrics};
use std::collections::BTreeMap;

#[test]
fn quality_average_is_the_mean_of_every_field() {
    let judgments = BTreeMap::from([
        ("best.rs".to_string(), 3),
        ("also-relevant.py".to_string(), 1),
    ]);
    let best_first = measure_query(
        &[
            "best.rs".to_string(),
            "noise.go".to_string(),
            "also-relevant.py".to_string(),
        ],
        &judgments,
    );
    let relevant_lower = measure_query(
        &[
            "noise.go".to_string(),
            "also-relevant.py".to_string(),
            "best.rs".to_string(),
        ],
        &judgments,
    );
    let mut sums = MetricSums::default();
    sums.push(&best_first);
    sums.push(&relevant_lower);
    assert_eq!(sums.count, 2);
    let average = sums.average();

    assert_eq!(
        average.recall_at_5,
        (best_first.recall_at_5 + relevant_lower.recall_at_5) / 2.0
    );
    assert_eq!(
        average.precision_at_10,
        (best_first.precision_at_10 + relevant_lower.precision_at_10) / 2.0
    );
    assert_eq!(
        average.ndcg_at_10,
        (best_first.ndcg_at_10 + relevant_lower.ndcg_at_10) / 2.0
    );
    for (got, (left, right)) in average.metric_values().zip(
        best_first
            .metric_values()
            .zip(relevant_lower.metric_values()),
    ) {
        assert_eq!(got, (left + right) / 2.0);
    }
}

#[test]
fn one_measured_query_averages_to_itself() {
    let judgments = BTreeMap::from([("same.rs".to_string(), 3)]);
    let metrics = measure_query(
        &[
            "same.rs".to_string(),
            "same.rs".to_string(),
            "noise.py".to_string(),
        ],
        &judgments,
    );
    let mut sums = MetricSums::default();
    sums.push(&metrics);
    assert_eq!(sums.average(), metrics);
}

#[test]
fn empty_quality_average_is_zero() {
    let sums = MetricSums::default();
    assert_eq!(sums.count, 0);
    assert_eq!(sums.average(), QualityMetrics::default());
}

#[test]
fn unanswerable_average_counts_queries_and_means_rates() {
    let judgments = BTreeMap::from([("a.rs".to_string(), 0), ("b.rs".to_string(), 0)]);
    let mut sums = UnanswerableMetricSums::default();
    sums.push(&["a.rs".to_string(), "b.rs".to_string()], &judgments);
    sums.push(&[], &judgments);
    assert_eq!(sums.count, 2);
    assert_eq!(
        sums.average(),
        UnanswerableMetrics {
            query_count: 2,
            explicit_negative_share_at_10: 0.5,
            judged_coverage_at_10: 0.5,
            empty_result_rate: 0.5,
        }
    );
}

#[test]
fn empty_unanswerable_average_is_zero() {
    let sums = UnanswerableMetricSums::default();
    assert_eq!(sums.count, 0);
    assert_eq!(sums.average(), UnanswerableMetrics::default());
}
