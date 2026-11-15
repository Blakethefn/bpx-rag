use super::super::search::{file_paths, QueryScores};
use super::super::*;
use super::RetrievalTally;
use crate::knowledge::query::KnowledgeSearchResult;
use crate::knowledge::KnowledgeStore;
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) fn record_relation_neighbors(
    store: &KnowledgeStore,
    results: &[KnowledgeSearchResult],
    judgments: &BTreeMap<String, u8>,
    tally: &mut RetrievalTally,
) -> Result<()> {
    let expanded = crate::search::attach_relation_neighbors(
        store,
        results.to_vec(),
        knowledge::links::RELATION_NEIGHBOR_LIMIT,
        TOP_K,
    )?;
    if expanded.len() != results.len() || expanded.iter().any(|result| result.why.graph_neighbor) {
        tally.queries_with_neighbors += 1;
    }
    tally
        .with_relations
        .push(&measure_query(&file_paths(&expanded), judgments));
    Ok(())
}

pub(super) fn record_query_totals(
    tally: &mut RetrievalTally,
    query: &EvalQuery,
    scores: &QueryScores,
    judgments: &BTreeMap<String, u8>,
) -> Option<UnanswerableMetrics> {
    if query.answerability == QueryAnswerability::UnanswerableInCorpus {
        tally
            .unanswerable_hybrid
            .push(&scores.hybrid_paths, judgments);
        tally
            .unanswerable_lexical
            .push(&scores.lexical_paths, judgments);
        tally
            .unanswerable_vector
            .push(&scores.vector_paths, judgments);
        Some(measure_unanswerable_query(&scores.hybrid_paths, judgments))
    } else {
        tally.overall.push(&scores.hybrid);
        tally.lexical_only.push(&scores.lexical);
        tally.vector_only.push(&scores.vector);
        tally
            .categories
            .entry(query.category.clone())
            .or_default()
            .push(&scores.hybrid);
        tally
            .languages
            .entry(query.language.clone())
            .or_default()
            .push(&scores.hybrid);
        None
    }
}
