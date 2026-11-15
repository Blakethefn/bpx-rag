use super::*;
use std::collections::BTreeMap;

mod citation;
mod unanswerable;
pub(in crate::eval) use citation::{citation_quality, normalize_citation_text};
pub(in crate::eval) use unanswerable::{measure_unanswerable_query, UnanswerableMetricSums};

#[derive(Clone, Copy, Default)]
pub(in crate::eval) struct MetricSums {
    total: QualityMetrics,
    pub(in crate::eval) count: usize,
}

impl MetricSums {
    pub(in crate::eval) fn push(&mut self, metrics: &QualityMetrics) {
        self.total.accumulate(metrics);
        self.count += 1;
    }

    pub(in crate::eval) fn average(mut self) -> QualityMetrics {
        self.total.mean(self.count);
        self.total
    }
}

pub(in crate::eval) fn measure_query(
    ranked_paths: &[String],
    judgments: &BTreeMap<String, u8>,
) -> QualityMetrics {
    let unique_paths = unique_paths(ranked_paths, usize::MAX);
    let relevant_total = judgments
        .values()
        .filter(|grade| **grade > 0)
        .count()
        .max(1) as f64;
    let relevant_at = |cutoff: usize| {
        unique_paths
            .iter()
            .take(cutoff)
            .filter(|path| judgments.get(path.as_str()).is_some_and(|grade| *grade > 0))
            .count()
    };
    let judged_coverage_at = |cutoff: usize| {
        let returned = unique_paths.len().min(cutoff);
        if returned == 0 {
            return 0.0;
        }
        let judged = unique_paths
            .iter()
            .take(cutoff)
            .filter(|path| judgments.contains_key(path.as_str()))
            .count();
        judged as f64 / returned as f64
    };
    let mrr_at_10 = unique_paths
        .iter()
        .take(TOP_K)
        .position(|path| judgments.get(path.as_str()).is_some_and(|grade| *grade > 0))
        .map_or(0.0, |rank| 1.0 / (rank as f64 + 1.0));
    let actual_dcg = dcg(unique_paths
        .iter()
        .take(TOP_K)
        .map(|path| judgments.get(path.as_str()).copied().unwrap_or(0)));
    let mut ideal_grades = judgments
        .values()
        .copied()
        .filter(|grade| *grade > 0)
        .collect::<Vec<_>>();
    ideal_grades.sort_by(|left, right| right.cmp(left));
    let ideal_dcg = dcg(ideal_grades.into_iter().take(TOP_K));
    let found_at_5 = relevant_at(5);
    QualityMetrics {
        recall_at_5: found_at_5 as f64 / relevant_total,
        recall_at_10: relevant_at(TOP_K) as f64 / relevant_total,
        precision_at_5: found_at_5 as f64 / 5.0,
        precision_at_10: relevant_at(TOP_K) as f64 / TOP_K as f64,
        judged_coverage_at_5: judged_coverage_at(5),
        judged_coverage_at_10: judged_coverage_at(TOP_K),
        mrr_at_10,
        ndcg_at_10: if ideal_dcg == 0.0 {
            0.0
        } else {
            actual_dcg / ideal_dcg
        },
        success_at_5: if found_at_5 > 0 { 1.0 } else { 0.0 },
    }
}

pub(in crate::eval) fn unique_paths(ranked_paths: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ranked_paths
        .iter()
        .filter(|path| seen.insert(path.as_str()))
        .take(limit)
        .cloned()
        .collect()
}

pub(in crate::eval) fn dcg(grades: impl Iterator<Item = u8>) -> f64 {
    grades
        .enumerate()
        .map(|(rank, grade)| {
            let gain = 2_f64.powi(i32::from(grade)) - 1.0;
            gain / ((rank as f64 + 2.0).log2())
        })
        .sum()
}

#[cfg(test)]
mod tests;
