//! Knowledge search: BM25, trigram, fuzzy, and vector layers fused with RRF.

use crate::embed;
use crate::knowledge::filter::{parse_filter, predicates_to_sql};
use crate::knowledge::KnowledgeStore;
use crate::search::profile::SearchWhy;
use anyhow::Result;
use std::collections::HashMap;

mod fuzzy;
mod lexical;
mod rank;
mod vector;
pub(crate) use fuzzy::VocabularyCache;
use fuzzy::*;
use lexical::*;
use rank::*;
use vector::*;

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
    if limit == 0 {
        return Ok(SearchReport::default());
    }
    if limit > MAX_CANDIDATE_CHUNKS {
        anyhow::bail!("knowledge search limit cannot exceed {MAX_CANDIDATE_CHUNKS}");
    }

    search_with_pool(
        store,
        query,
        filter,
        source_label,
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
    filter: Option<&str>,
    source_label: Option<&str>,
    limit: u32,
    candidate_limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<SearchReport> {
    let conn = store.conn();

    // Parse metadata filters
    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(super::filter::FilterPredicate::Source(label.to_string()));
    }

    let (meta_where, meta_params) = predicates_to_sql(&predicates, 0);

    let mut all_results: HashMap<i64, ScoredResult> = HashMap::new();

    if weights.keyword_weight > 0.0 {
        // Layer 1: FTS5 BM25
        let bm25 = knowledge_bm25_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
        merge_results(&mut all_results, &bm25, weights.keyword_weight, "bm25");

        // Search a wider chunk candidate pool before choosing the final
        // result limit. File diversity is applied after chunk-level RRF.
        if bm25.len() < candidate_limit as usize {
            let trigram =
                knowledge_trigram_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
            merge_results(
                &mut all_results,
                &trigram,
                weights.keyword_weight,
                "trigram",
            );
        }

        // Layer 3: Fuzzy (if lexical layers still leave candidate room)
        if all_results.len() < candidate_limit as usize {
            let fuzzy =
                knowledge_fuzzy_search(store, query, candidate_limit, &meta_where, &meta_params)?;
            merge_results(&mut all_results, &fuzzy, weights.keyword_weight, "fuzzy");
        }

        // Identifier words. This runs after fuzzy, so the fuzzy gate sees the same
        // pool it saw before this layer existed.
        let identifiers =
            knowledge_identifier_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
        merge_results(
            &mut all_results,
            &identifiers,
            weights.keyword_weight,
            "identifier",
        );
    }

    let mut warnings = Vec::new();

    // Layer 4: Vector similarity
    if weights.vector_weight > 0.0 {
        if let Some(emb) = store.embedder() {
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
                }
                Err(error) => warnings.push(format!(
                    "query embedding failed, so vector search was skipped: {error}"
                )),
            }
        }
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
        results: diversify_by_file(results, limit as usize),
        warnings,
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
mod tests;
