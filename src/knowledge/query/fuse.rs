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
use super::rank::{diversify_by_file, merge_results, ScoredResult};
use super::vector::knowledge_vector_search;
use super::{
    KnowledgeSearchResult, SearchReport, SearchScope, SearchWeights, IDENTIFIER_LAYER_WEIGHT,
};

/// Run the layered search with an explicit chunk candidate pool.
///
/// The pool size gates the lexical layers, so the requested output `limit`
/// must not change it. Callers that want a deeper ranking than the default
/// use the same pool as a shallower search, which keeps the top ranks equal.
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

    // Parse metadata filters
    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(FilterPredicate::Source(label.to_string()));
    }

    let (meta_where, meta_params) = predicates_to_sql(&predicates, 0);

    let mut all_results: HashMap<i64, ScoredResult> = HashMap::new();
    let mut layers = Vec::with_capacity(5);
    let pool_limit = candidate_limit as usize;

    if weights.keyword_weight > 0.0 {
        // Layer 1: FTS5 BM25. Always runs.
        let bm25 = knowledge_bm25_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
        merge_results(&mut all_results, &bm25, weights.keyword_weight, "bm25");
        layers.push(LayerReport::ran("bm25", bm25.len()));

        // Layer 2: trigram. It searches a wider chunk candidate pool before the final
        // result limit is chosen. File diversity is applied after chunk-level RRF.
        match trigram_skip(policy, query, bm25.len(), pool_limit) {
            Some(reason) => layers.push(LayerReport::skipped("trigram", reason)),
            None => {
                let trigram = knowledge_trigram_search(
                    conn,
                    query,
                    candidate_limit,
                    &meta_where,
                    &meta_params,
                )?;
                merge_results(
                    &mut all_results,
                    &trigram,
                    weights.keyword_weight,
                    "trigram",
                );
                layers.push(LayerReport::ran("trigram", trigram.len()));
            }
        }

        // Layer 3: fuzzy. The gate reads the pool the policy names.
        match fuzzy_skip(policy, bm25.len(), all_results.len(), pool_limit) {
            Some(reason) => layers.push(LayerReport::skipped("fuzzy", reason)),
            None => {
                match knowledge_fuzzy_search(
                    store,
                    query,
                    candidate_limit,
                    &meta_where,
                    &meta_params,
                )? {
                    Some(fuzzy) => {
                        merge_results(&mut all_results, &fuzzy, weights.keyword_weight, "fuzzy");
                        layers.push(LayerReport::ran("fuzzy", fuzzy.len()));
                    }
                    None => layers.push(LayerReport::skipped("fuzzy", SkipReason::NoCorrectedWord)),
                }
            }
        }

        // Identifier words. This runs after the gates, and no gate counts its results.
        if identifier_match_expression(query).is_empty() {
            layers.push(LayerReport::skipped(
                "identifier",
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
                "identifier",
            );
            layers.push(LayerReport::ran("identifier", identifiers.len()));
        }
    } else {
        for layer in ["bm25", "trigram", "fuzzy", "identifier"] {
            layers.push(LayerReport::skipped(layer, SkipReason::Disabled));
        }
    }

    let mut warnings = Vec::new();

    // Layer 4: vector similarity. Runs whenever an embedder is attached.
    if weights.vector_weight <= 0.0 {
        layers.push(LayerReport::skipped("vector", SkipReason::Disabled));
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
                    // ANN IDs come from the active model's full vector space, not
                    // lexical hits. SQL rechecks model, metadata, and exact scores.
                    candidates.as_deref(),
                )?;
                merge_results(&mut all_results, &vector, weights.vector_weight, "vector");
                layers.push(LayerReport::ran("vector", vector.len()));
            }
            Err(error) => {
                warnings.push(format!(
                    "query embedding failed, so vector search was skipped: {error}"
                ));
                layers.push(LayerReport::failed("vector", error.to_string()));
            }
        }
    } else {
        layers.push(LayerReport::skipped("vector", SkipReason::NoEmbedder));
    }

    // Sort by RRF score, then by chunk rowid, so ties do not depend on hash order.
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

/// The normalized query embedding. A vector is reused only when the same model already embedded
/// this exact query. Only a vector that passed normalization is cached, so failures are retried.
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
