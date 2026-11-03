//! Knowledge search: BM25, trigram, fuzzy, and vector layers fused with RRF.

use crate::embed;
use crate::knowledge::filter::{parse_filter, predicates_to_sql};
use crate::knowledge::KnowledgeStore;
use crate::search::profile::SearchWhy;
use anyhow::Result;
use std::collections::HashMap;

mod fuzzy;
mod layers;
mod lexical;
mod rank;
mod reranking;
mod vector;
pub(crate) use fuzzy::VocabularyCache;
use fuzzy::*;
use layers::{fuzzy_skip, trigram_skip};
pub use layers::{LayerOutcome, LayerPolicy, LayerReport, SkipReason};
use lexical::*;
use rank::*;
use vector::*;

/// Identifier layer weight, relative to the keyword weight. Chosen by the Stage 3 Revision 2
/// selection rule. It makes a precise identifier hit outrank a single lexical and vector pair.
const IDENTIFIER_LAYER_WEIGHT: f64 = 2.0;

/// Weight configuration for RRF layer blending
pub struct SearchWeights {
    pub keyword_weight: f64,
    pub vector_weight: f64,
}

impl Default for SearchWeights {
    fn default() -> Self {
        Self {
            keyword_weight: 1.0,
            vector_weight: 1.0,
        }
    }
}

/// The filters and gating policy for one search. Grouped so the policy entry point stays
/// within the argument limit.
#[derive(Debug, Clone, Copy)]
pub struct SearchScope<'a> {
    pub filter: Option<&'a str>,
    pub source_label: Option<&'a str>,
    pub policy: LayerPolicy,
    /// Whether the store's reranker, if one is attached, reorders this search's candidates.
    /// Diagnostics that measure one layer on its own pass `false` so their output is not reranked.
    pub rerank: bool,
}

/// A search result from the knowledge store (or combined search).
#[derive(Debug, serde::Serialize)]
pub struct KnowledgeSearchResult {
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub source_label: String,
    pub file_path: String,
    pub file_id: i64,
    pub source_id: i64,
    pub lines: String,
    pub metadata: serde_json::Value,
    pub source_type: String,
    pub why: SearchWhy,
}

/// Search the knowledge DB with optional metadata filtering.
///
/// Runs the full multi-layer pipeline: BM25 → trigram → fuzzy → vector → RRF.
/// Metadata filters restrict results before RRF fusion.
pub fn knowledge_search(
    store: &KnowledgeStore,
    query: &str,
    filter: Option<&str>,
    source_label: Option<&str>,
    limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<Vec<KnowledgeSearchResult>> {
    Ok(knowledge_search_report(
        store,
        query,
        filter,
        source_label,
        limit,
        weights,
        snippet_bytes,
    )?
    .results)
}

/// Ranked results plus notices about layers that degraded during the search.
#[derive(Debug, Default)]
pub struct SearchReport {
    pub results: Vec<KnowledgeSearchResult>,
    pub warnings: Vec<String>,
    /// Per-layer diagnostics, in pipeline order.
    pub layers: Vec<LayerReport>,
}

/// Like [`knowledge_search`], but also reports layers that were skipped.
pub fn knowledge_search_report(
    store: &KnowledgeStore,
    query: &str,
    filter: Option<&str>,
    source_label: Option<&str>,
    limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<SearchReport> {
    knowledge_search_report_with_policy(
        store,
        query,
        SearchScope {
            filter,
            source_label,
            policy: LayerPolicy::default(),
            rerank: true,
        },
        limit,
        weights,
        snippet_bytes,
    )
}

/// Like [`knowledge_search_report`], with an explicit gating policy. The retrieval
/// evaluation uses this to compare policies on the same index.
pub fn knowledge_search_report_with_policy(
    store: &KnowledgeStore,
    query: &str,
    scope: SearchScope<'_>,
    limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<SearchReport> {
    if limit == 0 {
        return Ok(SearchReport::default());
    }
    if limit > MAX_CANDIDATE_CHUNKS {
        anyhow::bail!("knowledge search limit cannot exceed {MAX_CANDIDATE_CHUNKS}");
    }

    search_with_pool(
        store,
        query,
        scope,
        limit,
        expanded_candidate_limit(limit),
        weights,
        snippet_bytes,
    )
}

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
        rerank,
    } = scope;
    let conn = store.conn();

    // Parse metadata filters
    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(super::filter::FilterPredicate::Source(label.to_string()));
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
        match emb
            .embed_one(query)
            .and_then(|vector| embed::normalize_embedding(&vector, emb.dim()))
        {
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
    // Rerank before file diversification, so the output cut sees the reranked order.
    if let Some(slot) = store.reranker().filter(|_| rerank) {
        layers.push(reranking::apply_reranker(
            slot,
            query,
            &mut entries,
            &mut warnings,
        ));
    }
    let results: Vec<KnowledgeSearchResult> = entries
        .into_iter()
        .map(|(_, sr)| sr.into_knowledge_result(snippet_bytes))
        .collect::<Result<Vec<_>>>()?;
    Ok(SearchReport {
        results: diversify_by_file(results, limit as usize),
        warnings,
        layers,
    })
}

const MAX_CANDIDATE_CHUNKS: u32 = 1_000;

pub const CANDIDATE_OVERSAMPLE_FACTOR: u32 = 5;

pub(crate) fn expanded_candidate_limit(limit: u32) -> u32 {
    limit
        .saturating_mul(CANDIDATE_OVERSAMPLE_FACTOR)
        .min(MAX_CANDIDATE_CHUNKS)
        .max(limit)
}

impl Clone for KnowledgeSearchResult {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            snippet: self.snippet.clone(),
            score: self.score,
            source_label: self.source_label.clone(),
            file_path: self.file_path.clone(),
            file_id: self.file_id,
            source_id: self.source_id,
            lines: self.lines.clone(),
            metadata: self.metadata.clone(),
            source_type: self.source_type.clone(),
            why: self.why.clone(),
        }
    }
}

// ── Search layers ────────────────────────────────────────────────────────────

#[cfg(test)]
mod fuzzy_bench;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod reranking_tests;
#[cfg(test)]
mod tests;
