use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

mod embeddings;
mod knowledge;
mod search;

pub use embeddings::{EmbeddingDevice, EmbeddingsConfig};
pub use knowledge::{ContextConfig, KnowledgeConfig};
pub use search::SearchConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub search: SearchConfig,
    pub fetch: FetchConfig,
    pub integration: IntegrationConfig,
    pub cleanup: CleanupConfig,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub max_stdout_bytes: usize,
    pub head_ratio: f64,
    #[serde(default = "default_preview_threshold_bytes")]
    pub preview_threshold_bytes: usize,
}

fn default_preview_threshold_bytes() -> usize {
    8192
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FetchConfig {
    pub cache_ttl_hours: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IntegrationConfig {
    pub taskvault_bin: String,
    pub vault_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CleanupConfig {
    pub stale_db_days: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                max_stdout_bytes: 102_400,
                head_ratio: 0.6,
                preview_threshold_bytes: default_preview_threshold_bytes(),
            },
            search: SearchConfig {
                default_limit: 15,
                throttle_max: 8,
                throttle_window_secs: 60,
                snippet_bytes: search::default_snippet_bytes(),
                secondary_snippet_bytes: search::default_secondary_snippet_bytes(),
                top_result_count: search::default_top_result_count(),
                relation_neighbors: search::default_relation_neighbors(),
                vector_weight: 1.0,
                keyword_weight: 1.0,
            },
            fetch: FetchConfig {
                cache_ttl_hours: 24,
            },
            integration: IntegrationConfig {
                taskvault_bin: "taskvault".to_string(),
                vault_path: dirs::home_dir()
                    .map(|h| h.join("obsidian-vault").to_string_lossy().to_string())
                    .unwrap_or_else(|| "~/obsidian-vault".to_string()),
            },
            cleanup: CleanupConfig { stale_db_days: 14 },
            embeddings: EmbeddingsConfig::default(),
            context: ContextConfig::default(),
            knowledge: KnowledgeConfig::default(),
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not find config directory")?;
        Ok(crate::db::app_dir_under(&base)?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        let config: Config = toml::from_str(&content).context("Failed to parse config.toml")?;
        Ok(config)
    }

    pub fn init_default() -> Result<()> {
        let path = Self::config_path()?;
        if path.exists() {
            println!("Config already exists at {}", path.display());
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let default = Config::default();
        let content = toml::to_string_pretty(&default)?;
        fs::write(&path, &content)?;
        println!("Created default config at {}", path.display());
        Ok(())
    }
}
