use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextConfig {
    pub budget_tokens: u64,
    pub stale_threshold_minutes: u64,
    #[serde(default = "default_recency_weight")]
    pub recency_weight: f64,
    #[serde(default = "default_frequency_weight")]
    pub frequency_weight: f64,
    #[serde(default = "default_staleness_weight")]
    pub staleness_weight: f64,
}

fn default_recency_weight() -> f64 {
    0.4
}
fn default_frequency_weight() -> f64 {
    0.3
}
fn default_staleness_weight() -> f64 {
    0.3
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            budget_tokens: 200_000,
            stale_threshold_minutes: 30,
            recency_weight: 0.4,
            frequency_weight: 0.3,
            staleness_weight: 0.3,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnowledgeConfig {
    #[serde(default = "default_knowledge_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub db_path: Option<String>,
    #[serde(default = "default_sync_stale_minutes")]
    pub sync_stale_minutes: u64,
    #[serde(default)]
    pub sources: Vec<KnowledgeSourceConfig>,
}

fn default_knowledge_enabled() -> bool {
    true
}

fn default_sync_stale_minutes() -> u64 {
    60
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            db_path: None,
            sync_stale_minutes: 60,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSourceConfig {
    pub label: String,
    pub path: String,
    pub glob: Option<String>,
    #[serde(default)]
    pub enrichments: Vec<String>,
}
