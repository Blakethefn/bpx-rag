use super::*;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs::{self};
use std::path::Path;

/// Running total of one [`QualityMetrics`] record, plus how many samples were added.
#[derive(Clone, Copy, Default)]
pub(in crate::eval) struct MetricSums {
    total: QualityMetrics,
    pub(in crate::eval) count: usize,
}

/// Running total of one [`UnanswerableMetrics`] record. `count` becomes `query_count`.
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

pub(in crate::eval) fn citation_quality(
    corpus_dir: &Path,
    file_path: &str,
    range: &str,
    snippet: &str,
) -> Result<(bool, bool)> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Ok((false, false));
    }
    let file_path = corpus_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("search result cited a missing file {}", file_path.display()))?;
    let Some((start, end)) = range.split_once('-') else {
        return Ok((false, false));
    };
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return Ok((false, false));
    };
    let lines = content.lines().collect::<Vec<_>>();
    if start == 0 || end < start || end > lines.len() as u32 {
        return Ok((false, false));
    }
    let cited_lines = lines[(start - 1) as usize..end as usize].join("\n");
    let snippet = snippet.strip_suffix("...").unwrap_or(snippet);
    let normalized_citation = normalize_citation_text(&cited_lines);
    let normalized_snippet = normalize_citation_text(snippet);
    let content_matches =
        !normalized_snippet.is_empty() && normalized_citation.starts_with(&normalized_snippet);
    Ok((true, content_matches))
}

pub(in crate::eval) fn normalize_citation_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
