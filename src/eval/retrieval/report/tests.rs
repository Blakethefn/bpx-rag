use super::{QualityMetrics, UnanswerableMetrics};

#[test]
fn summed_rates_are_the_serialized_f64_fields() {
    let quality = QualityMetrics {
        recall_at_5: 0.5,
        recall_at_10: 1.5,
        precision_at_5: 2.5,
        precision_at_10: 3.5,
        judged_coverage_at_5: 4.5,
        judged_coverage_at_10: 5.5,
        mrr_at_10: 6.5,
        ndcg_at_10: 7.5,
        success_at_5: 8.5,
    };
    assert_eq!(sorted_f64_fields(&quality), sorted(quality.metric_values()));
    assert_eq!(
        object_keys(&quality),
        names(&[
            "judged_coverage_at_10",
            "judged_coverage_at_5",
            "mrr_at_10",
            "ndcg_at_10",
            "precision_at_10",
            "precision_at_5",
            "recall_at_10",
            "recall_at_5",
            "success_at_5",
        ])
    );

    let unanswerable = UnanswerableMetrics {
        query_count: 7,
        explicit_negative_share_at_10: 0.25,
        judged_coverage_at_10: 0.5,
        empty_result_rate: 0.75,
    };
    assert_eq!(
        sorted_f64_fields(&unanswerable),
        sorted(unanswerable.metric_values())
    );
    assert_eq!(
        object_keys(&unanswerable),
        names(&[
            "empty_result_rate",
            "explicit_negative_share_at_10",
            "judged_coverage_at_10",
            "query_count",
        ])
    );
    let serialized = serde_json::to_value(unanswerable).expect("unanswerable metrics serialize");
    assert_eq!(serialized["query_count"], 7);
    assert!(!serialized["query_count"].is_f64());
}

fn names(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_string()).collect()
}

fn object_keys<T: serde::Serialize>(record: &T) -> Vec<String> {
    let mut keys = serde_json::to_value(record)
        .expect("metric record serializes")
        .as_object()
        .expect("metric record serializes to an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn sorted(values: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut values = values.collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values
}

fn sorted_f64_fields<T: serde::Serialize>(record: &T) -> Vec<f64> {
    let value = serde_json::to_value(record).expect("metric record serializes");
    let mut values = value
        .as_object()
        .expect("metric record serializes to an object")
        .values()
        .filter(|value| value.is_f64())
        .map(|value| value.as_f64().expect("f64 metric"))
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values
}
