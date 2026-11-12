use std::collections::HashMap;

use anyhow::Result;

use crate::embed;
use crate::knowledge::filter::{parse_filter, predicates_to_sql, FilterPredicate};
use crate::knowledge::KnowledgeStore;

use super::fuzzy::knowledge_fuzzy_search;
use super::layers::{fuzzy_skip, trigram_skip, LayerReport, SkipReason};
use super::lexical::{
    identifier_match_expression, knowledge_bm25_search, knowledge_identifier_search,
    knowledge_trigram_search,
};
use super::rank::{
    diversify_by_file, keyword_layers, merge_results, ScoredResult, BM25, FUZZY, IDENTIFIER,
    TRIGRAM, VECTOR,
};
use super::vector::knowledge_vector_search;
use super::{
    KnowledgeSearchResult, SearchReport, SearchScope, SearchWeights, IDENTIFIER_LAYER_WEIGHT,
};

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
    let conn = store.conn();

    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(FilterPredicate::Source(label.to_string()));
    }

    let (meta_where, meta_params) = predicates_to_sql(&predicates, 0);

    let mut all_results: HashMap<i64, ScoredResult> = HashMap::new();
    let mut layers = Vec::with_capacity(5);
    let pool_limit = candidate_limit as usize;

    if weights.keyword_weight > 0.0 {
        let bm25 = knowledge_bm25_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
        merge_results(&mut all_results, &bm25, weights.keyword_weight, BM25);
        layers.push(LayerReport::ran(BM25.name, bm25.len()));

        match trigram_skip(policy, query, bm25.len(), pool_limit) {
            Some(reason) => layers.push(LayerReport::skipped(TRIGRAM.name, reason)),
            None => {
                let trigram = knowledge_trigram_search(
                    conn,
                    query,
                    candidate_limit,
                    &meta_where,
                    &meta_params,
                )?;
                merge_results(&mut all_results, &trigram, weights.keyword_weight, TRIGRAM);
                layers.push(LayerReport::ran(TRIGRAM.name, trigram.len()));
            }
        }

        match fuzzy_skip(policy, bm25.len(), all_results.len(), pool_limit) {
            Some(reason) => layers.push(LayerReport::skipped(FUZZY.name, reason)),
            None => {
                match knowledge_fuzzy_search(
                    store,
                    query,
                    candidate_limit,
                    &meta_where,
                    &meta_params,
                )? {
                    Some(fuzzy) => {
                        merge_results(&mut all_results, &fuzzy, weights.keyword_weight, FUZZY);
                        layers.push(LayerReport::ran(FUZZY.name, fuzzy.len()));
                    }
                    None => layers.push(LayerReport::skipped(
                        FUZZY.name,
                        SkipReason::NoCorrectedWord,
                    )),
                }
            }
        }

        if identifier_match_expression(query).is_empty() {
            layers.push(LayerReport::skipped(
                IDENTIFIER.name,
                SkipReason::TooFewContentWords,
            ));
        } else {
            let identifiers = knowledge_identifier_search(
                conn,
                query,
                candidate_limit,
                &meta_where,
                &meta_params,
            )?;
            merge_results(
                &mut all_results,
                &identifiers,
                weights.keyword_weight * IDENTIFIER_LAYER_WEIGHT,
                IDENTIFIER,
            );
            layers.push(LayerReport::ran(IDENTIFIER.name, identifiers.len()));
        }
    } else {
        for layer in keyword_layers() {
            layers.push(LayerReport::skipped(layer.name, SkipReason::Disabled));
        }
    }

    let mut warnings = Vec::new();

    if weights.vector_weight <= 0.0 {
        layers.push(LayerReport::skipped(VECTOR.name, SkipReason::Disabled));
    } else if let Some(emb) = store.embedder() {
        match cached_query_embedding(store, emb.as_ref(), query) {
            Ok(query_embedding) => {
                let candidates = store.vector_candidates(
                    emb.model_id(),
                    &query_embedding,
                    candidate_limit,
                    &predicates,
                )?;
                let vector = knowledge_vector_search(
                    conn,
                    &query_embedding,
                    candidate_limit,
                    &meta_where,
                    &meta_params,
                    emb.model_id(),
                    candidates.as_deref(),
                )?;
                merge_results(&mut all_results, &vector, weights.vector_weight, VECTOR);
                layers.push(LayerReport::ran(VECTOR.name, vector.len()));
            }
            Err(error) => {
                warnings.push(format!(
                    "query embedding failed, so vector search was skipped: {error}"
                ));
                layers.push(LayerReport::failed(VECTOR.name, error.to_string()));
            }
        }
    } else {
        layers.push(LayerReport::skipped(VECTOR.name, SkipReason::NoEmbedder));
    }

    let mut entries: Vec<(i64, ScoredResult)> = all_results.into_iter().collect();
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
