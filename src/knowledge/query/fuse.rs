use anyhow::Result;

use crate::embed;
use crate::knowledge::filter::{parse_filter, predicates_to_sql, FilterPredicate};
use crate::knowledge::KnowledgeStore;

use super::rank::diversify_by_file;
use super::{KnowledgeSearchResult, SearchReport, SearchScope, SearchWeights};

mod layers;

use layers::search_layers;

pub(crate) fn search_with_pool(
    store: &KnowledgeStore,
    query: &str,
    scope: SearchScope<'_>,
    limit: u32,
    candidate_limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<SearchReport> {
    let SearchScope {
        filter,
        source_label,
        policy,
        max_per_file,
    } = scope;

    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(FilterPredicate::Source(label.to_string()));
    }

    let (meta_where, meta_params) = predicates_to_sql(&predicates, 0);
    let (all_results, layers, warnings) = search_layers(
        store,
        query,
        policy,
        candidate_limit,
        weights,
        &meta_where,
        &meta_params,
        &predicates,
    )?;

    let mut entries: Vec<(i64, super::rank::ScoredResult)> = all_results.into_iter().collect();
    entries.sort_by(|(left_id, left), (right_id, right)| {
        right
            .rrf_score
            .partial_cmp(&left.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left_id.cmp(right_id))
    });
    let results: Vec<KnowledgeSearchResult> = entries
        .into_iter()
        .map(|(_, sr)| sr.into_knowledge_result(snippet_bytes))
        .collect::<Result<Vec<_>>>()?;
    Ok(SearchReport {
        results: diversify_by_file(results, limit as usize, max_per_file as usize),
        warnings,
        layers,
    })
}

fn cached_query_embedding(
    store: &KnowledgeStore,
    embedder: &dyn embed::Embed,
    query: &str,
) -> Result<Vec<f32>> {
    let cache = store.query_embedding_cache();
    if let Some(vector) = cache.get(embedder.model_id(), query) {
        return Ok(vector);
    }
    let vector = embedder
        .embed_one(query)
        .and_then(|vector| embed::normalize_embedding(&vector, embedder.dim()))?;
    cache.insert(embedder.model_id(), query, vector.clone());
    Ok(vector)
}
