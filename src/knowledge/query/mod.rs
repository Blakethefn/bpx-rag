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
#[cfg(test)]
pub use layers::SkipReason;
pub use layers::{LayerOutcome, LayerPolicy, LayerReport};

const IDENTIFIER_LAYER_WEIGHT: f64 = 2.0;

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

#[derive(Debug, Clone, Copy)]
pub struct SearchScope<'a> {
    pub filter: Option<&'a str>,
    pub source_label: Option<&'a str>,
    pub policy: LayerPolicy,
    pub max_per_file: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
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

#[derive(Debug, Default)]
pub struct SearchReport {
    pub results: Vec<KnowledgeSearchResult>,
    pub warnings: Vec<String>,
    pub layers: Vec<LayerReport>,
}

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

pub const MAX_PER_FILE: u32 = 50;

pub const CANDIDATE_OVERSAMPLE_FACTOR: u32 = 5;

pub(crate) fn expanded_candidate_limit(limit: u32) -> u32 {
    limit
        .saturating_mul(CANDIDATE_OVERSAMPLE_FACTOR)
        .min(MAX_CANDIDATE_CHUNKS)
        .max(limit)
}

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;
