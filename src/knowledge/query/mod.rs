//! Knowledge search: BM25, trigram, fuzzy, and vector layers fused with RRF.

#[cfg(test)]
use crate::embed;
use crate::knowledge::KnowledgeStore;
use crate::search::profile::SearchWhy;
use anyhow::Result;

mod fuse;
mod fuzzy;
mod layers;
mod lexical;
mod rank;
mod vector;
pub(crate) use fuse::search_with_pool;
pub(crate) use fuzzy::VocabularyCache;
#[allow(unused_imports)]
use fuzzy::*;
pub use layers::{LayerOutcome, LayerPolicy, LayerReport};
#[cfg(test)]
pub use layers::SkipReason;
#[allow(unused_imports)]
use lexical::*;
#[allow(unused_imports)]
use rank::*;
#[allow(unused_imports)]
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
    /// Most passages kept from one file before other files fill the limit. Must be at least 1.
    pub max_per_file: u32,
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
            max_per_file: 1,
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
    if !(1..=MAX_PER_FILE).contains(&scope.max_per_file) {
        anyhow::bail!("max_per_file must be between 1 and {MAX_PER_FILE}");
    }
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

const MAX_CANDIDATE_CHUNKS: u32 = 1_000;

/// Largest accepted `max_per_file`. The CLI, MCP, and library all enforce 1 through this value.
pub const MAX_PER_FILE: u32 = 50;

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
mod bench;
#[cfg(test)]
mod tests;
