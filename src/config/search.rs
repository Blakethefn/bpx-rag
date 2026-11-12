use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_limit: u32,
    pub throttle_max: u32,
    pub throttle_window_secs: u64,
    #[serde(default = "default_snippet_bytes")]
    pub snippet_bytes: usize,
    #[serde(default = "default_secondary_snippet_bytes")]
    pub secondary_snippet_bytes: usize,
    #[serde(default = "default_top_result_count")]
    pub top_result_count: usize,
    #[serde(default = "default_relation_neighbors")]
    pub relation_neighbors: usize,
    #[serde(default = "default_weight")]
    pub vector_weight: f64,
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
