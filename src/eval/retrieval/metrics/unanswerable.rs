use super::super::super::TOP_K;
use super::super::UnanswerableMetrics;
use super::unique_paths;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default)]
pub(in crate::eval) struct UnanswerableMetricSums {
    total: UnanswerableMetrics,
    pub(in crate::eval) count: usize,
}

impl UnanswerableMetricSums {
    pub(in crate::eval) fn push(
        &mut self,
        ranked_paths: &[String],
        judgments: &BTreeMap<String, u8>,
    ) {
        self.total
            .accumulate(&measure_unanswerable_query(ranked_paths, judgments));
        self.count += 1;
    }

    pub(in crate::eval) fn average(mut self) -> UnanswerableMetrics {
        self.total.mean(self.count);
        self.total.query_count = self.count;
        self.total
    }
}

pub(in crate::eval) fn measure_unanswerable_query(
    ranked_paths: &[String],
    judgments: &BTreeMap<String, u8>,
) -> UnanswerableMetrics {
    let top_paths = unique_paths(ranked_paths, TOP_K);
    let returned = top_paths.len();
    let explicitly_negative = top_paths
        .iter()
        .filter(|path| judgments.get(path.as_str()) == Some(&0))
        .count();
    let judged = top_paths
        .iter()
        .filter(|path| judgments.contains_key(path.as_str()))
        .count();
    UnanswerableMetrics {
        query_count: 1,
        explicit_negative_share_at_10: if returned == 0 {
            0.0
        } else {
            explicitly_negative as f64 / returned as f64
        },
        judged_coverage_at_10: if returned == 0 {
            0.0
        } else {
            judged as f64 / returned as f64
        },
        empty_result_rate: (returned == 0) as u8 as f64,
    }
}
