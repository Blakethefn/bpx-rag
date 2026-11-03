//! Reranking of the fused candidate list, before file diversification.

use super::layers::{LayerReport, SkipReason};
use super::rank::ScoredResult;
use crate::rerank::{order_by_score, RerankSlot};
use crate::search::profile::SearchWhy;

/// Fewest fused candidates worth reordering.
const MIN_RERANK_CANDIDATES: usize = 2;

/// Reorder the top of the fused list with the store's reranker. Returns the layer diagnostic and
/// appends a warning on failure. When reranking fails, `entries` keeps the fused order.
pub(super) fn apply_reranker(
    slot: &RerankSlot,
    query: &str,
    entries: &mut Vec<(i64, ScoredResult)>,
    warnings: &mut Vec<String>,
) -> LayerReport {
    let head_len = slot.candidates.min(entries.len());
    if head_len < MIN_RERANK_CANDIDATES {
        return LayerReport::skipped("rerank", SkipReason::TooFewCandidates);
    }
    let reranker = match &slot.reranker {
        Ok(reranker) => reranker,
        Err(reason) => {
            warnings.push(format!(
                "reranker unavailable, so the fused order was kept: {reason}"
            ));
            return LayerReport::failed("rerank", reason.clone());
        }
    };
    let passages: Vec<&str> = entries[..head_len]
        .iter()
        .map(|(_, result)| result.content.as_str())
        .collect();
    let scored = reranker
        .score(query, &passages)
        .and_then(|scores| order_by_score(&scores, head_len));
    match scored {
        Ok(order) => {
            reorder_head(entries, &order);
            LayerReport::ran("rerank", head_len)
        }
        Err(error) => {
            let message = format!("{error:#}");
            warnings.push(format!(
                "reranker failed, so the fused order was kept: {message}"
            ));
            LayerReport::failed("rerank", message)
        }
    }
}

/// Put the head in reranked order. Each position keeps the RRF score of the slot it takes, so the
/// scores stay descending and the caller's sort keeps the reranked order. The reranker's score
/// moves into `why`, where `explain` shows it.
fn reorder_head(entries: &mut Vec<(i64, ScoredResult)>, order: &[(usize, f32)]) {
    let tail = entries.split_off(order.len());
    let head = std::mem::take(entries);
    let slots: Vec<f64> = head.iter().map(|(_, result)| result.rrf_score).collect();

    let mut position_of = vec![0; order.len()];
    for (position, (input_index, _)) in order.iter().enumerate() {
        position_of[*input_index] = position;
    }
    let mut indexed: Vec<(usize, (i64, ScoredResult))> = head.into_iter().enumerate().collect();
    indexed.sort_by_key(|(input_index, _)| position_of[*input_index]);

    let reordered = indexed
        .into_iter()
        .zip(order)
        .zip(slots)
        .map(|(((_, (rowid, result)), (_, rerank_score)), slot)| {
            let why = SearchWhy {
                rerank_score: Some(*rerank_score),
                rrf_score: Some(result.rrf_score),
                ..result.why
            };
            (
                rowid,
                ScoredResult {
                    rrf_score: slot,
                    why,
                    ..result
                },
            )
        })
        .collect::<Vec<_>>();
    entries.extend(reordered);
    entries.extend(tail);
}
