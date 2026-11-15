use std::collections::HashMap;

use anyhow::Result;

use crate::knowledge::filter::FilterPredicate;
use crate::knowledge::KnowledgeStore;

use super::super::fuzzy::knowledge_fuzzy_search;
use super::super::layers::{fuzzy_skip, trigram_skip, LayerPolicy, LayerReport, SkipReason};
use super::super::lexical::{
    identifier_match_expression, knowledge_bm25_search, knowledge_identifier_search,
    knowledge_trigram_search,
};
use super::super::rank::{
    keyword_layers, merge_results, ScoredResult, BM25, FUZZY, IDENTIFIER, TRIGRAM, VECTOR,
};
use super::super::vector::knowledge_vector_search;
use super::super::{SearchWeights, IDENTIFIER_LAYER_WEIGHT};
use super::cached_query_embedding;

pub(super) fn search_layers(
    store: &KnowledgeStore,
    query: &str,
    policy: LayerPolicy,
    candidate_limit: u32,
    weights: &SearchWeights,
    meta_where: &str,
    meta_params: &[String],
    predicates: &[FilterPredicate],
) -> Result<(HashMap<i64, ScoredResult>, Vec<LayerReport>, Vec<String>)> {
    let conn = store.conn();
    let mut all_results: HashMap<i64, ScoredResult> = HashMap::new();
    let mut layers = Vec::with_capacity(5);
    let pool_limit = candidate_limit as usize;

    if weights.keyword_weight > 0.0 {
        let bm25 = knowledge_bm25_search(conn, query, candidate_limit, meta_where, meta_params)?;
        merge_results(&mut all_results, &bm25, weights.keyword_weight, BM25);
        layers.push(LayerReport::ran(BM25.name, bm25.len()));

        match trigram_skip(policy, query, bm25.len(), pool_limit) {
            Some(reason) => layers.push(LayerReport::skipped(TRIGRAM.name, reason)),
            None => {
                let trigram = knowledge_trigram_search(
                    conn,
                    query,
                    candidate_limit,
                    meta_where,
                    meta_params,
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
                    meta_where,
                    meta_params,
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
            let identifiers =
                knowledge_identifier_search(conn, query, candidate_limit, meta_where, meta_params)?;
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
                    predicates,
                )?;
                let vector = knowledge_vector_search(
                    conn,
                    &query_embedding,
                    candidate_limit,
                    meta_where,
                    meta_params,
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

    Ok((all_results, layers, warnings))
}
