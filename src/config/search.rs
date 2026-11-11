use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Max results per query (default 15)
    pub default_limit: u32,
    /// Max searches per throttle window (default 8)
    pub throttle_max: u32,
    /// Throttle window in seconds (default 60)
    pub throttle_window_secs: u64,
    /// Max bytes per snippet for top results (default 2000)
    #[serde(default = "default_snippet_bytes")]
    pub snippet_bytes: usize,
    /// Max bytes per snippet for lower-ranked results (default 800)
    #[serde(default = "default_secondary_snippet_bytes")]
    pub secondary_snippet_bytes: usize,
    /// Number of top-ranked results that get the larger snippet size (default 3)
    #[serde(default = "default_top_result_count")]
    pub top_result_count: usize,
    /// Linked files from the top hit's relations to merge into default results;
    /// 0 turns relation neighbors off (default 2)
    #[serde(default = "default_relation_neighbors")]
    pub relation_neighbors: usize,
    /// Multiplier for vector (semantic) RRF scores (default 1.0)
    #[serde(default = "default_weight")]
    pub vector_weight: f64,
    /// Multiplier for keyword (BM25/trigram/fuzzy) RRF scores (default 1.0)
    #[serde(default = "default_weight")]
    pub keyword_weight: f64,
}

pub(super) fn default_snippet_bytes() -> usize {
    2000
}

pub(super) fn default_secondary_snippet_bytes() -> usize {
    800
}

pub(super) fn default_top_result_count() -> usize {
    3
}

pub(super) fn default_relation_neighbors() -> usize {
    crate::knowledge::links::RELATION_NEIGHBOR_LIMIT
}

fn default_weight() -> f64 {
    1.0
}
