use std::collections::HashMap;

use crate::search::profile::SearchWhy;

use super::{RankedResult, RetrievalLayer, ScoredResult};

pub(in super::super) fn merge_results(
    acc: &mut HashMap<i64, ScoredResult>,
    results: &[RankedResult],
    weight: f64,
    layer: RetrievalLayer,
) {
    const K: f64 = 60.0;

    for (rank, result) in results.iter().enumerate() {
        let rrf_score = weight * (1.0 / (K + rank as f64 + 1.0));

        acc.entry(result.rowid)
            .and_modify(|sr| {
                sr.rrf_score += rrf_score;
                layer.add(&mut sr.why, rrf_score);
            })
            .or_insert_with(|| {
                let mut why = SearchWhy::default();
                layer.add(&mut why, rrf_score);
                ScoredResult {
                    file_id: result.file_id,
                    source_id: result.source_id,
                    title: result.title.clone(),
                    content: result.content.clone(),
                    source_label: result.source_label.clone(),
                    file_path: result.file_path.clone(),
                    line_start: result.line_start,
                    line_end: result.line_end,
                    metadata: result.metadata.clone(),
                    rrf_score,
                    why,
                }
            });
    }
}
