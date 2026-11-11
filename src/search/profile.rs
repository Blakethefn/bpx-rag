use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

mod why;

pub use why::SearchWhy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchProfile {
    Default,
    CodeReview,
    Debug,
    Planning,
    VaultNavigation,
}

impl SearchProfile {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("") => Ok(Self::Default),
            Some("code_review") => Ok(Self::CodeReview),
            Some("debug") => Ok(Self::Debug),
            Some("planning") => Ok(Self::Planning),
            Some("vault_navigation") => Ok(Self::VaultNavigation),
            Some(other) => Err(anyhow!(
                "Unknown profile '{other}'. Expected one of: code_review, debug, planning, vault_navigation"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::CodeReview => "code_review",
            Self::Debug => "debug",
            Self::Planning => "planning",
            Self::VaultNavigation => "vault_navigation",
        }
    }

    pub fn is_explicit(self) -> bool {
        !matches!(self, Self::Default)
    }

    pub fn effective_query(self, query: &str) -> String {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        match self {
            Self::Default | Self::CodeReview => trimmed.to_string(),
            Self::Debug => {
                let lower = trimmed.to_lowercase();
                let mut extras = Vec::new();
                if lower.contains("panic") || lower.contains("exception") || lower.contains("error")
                {
                    extras.push("failure");
                    extras.push("root cause");
                }
                if lower.contains("trace") || lower.contains("stack") {
                    extras.push("stack trace");
                }
                if extras.is_empty() {
                    trimmed.to_string()
                } else {
                    format!("{trimmed} {}", extras.join(" "))
                }
            }
            Self::Planning => format!("{trimmed} architecture plan steps"),
            Self::VaultNavigation => format!("{trimmed} related notes links"),
        }
    }

    pub fn keyword_weight_multiplier(self) -> f64 {
        match self {
            Self::Default => 1.0,
            Self::CodeReview => 1.35,
            Self::Debug => 1.1,
            Self::Planning => 1.0,
            Self::VaultNavigation => 0.95,
        }
    }

    pub fn vector_weight_multiplier(self) -> f64 {
        match self {
            Self::Default => 1.0,
            Self::CodeReview => 0.9,
            Self::Debug => 1.0,
            Self::Planning => 1.1,
            Self::VaultNavigation => 1.2,
        }
    }

    pub fn top_snippet_override(self) -> Option<usize> {
        match self {
            Self::Default => None,
            Self::CodeReview => Some(1400),
            Self::Debug => Some(1800),
            Self::Planning => Some(2400),
            Self::VaultNavigation => Some(1200),
        }
    }

    pub fn secondary_snippet_override(self) -> Option<usize> {
        match self {
            Self::Default => None,
            Self::CodeReview => Some(600),
            Self::Debug => Some(900),
            Self::Planning => Some(1400),
            Self::VaultNavigation => Some(700),
        }
    }

    pub fn knowledge_bias_multiplier(self) -> f64 {
        match self {
            Self::Default => 1.0,
            Self::CodeReview => 1.0,
            Self::Debug => 1.05,
            Self::Planning => 1.15,
            Self::VaultNavigation => 1.25,
        }
    }

    pub fn graph_boost(self, scoped_filtering: bool) -> f64 {
        match self {
            Self::Default => 0.0,
            Self::CodeReview => {
                if scoped_filtering {
                    0.0015
                } else {
                    0.0
                }
            }
            Self::Debug => 0.0015,
            Self::Planning => 0.003,
            Self::VaultNavigation => 0.0045,
        }
    }

    pub fn inject_neighbors(self) -> bool {
        matches!(self, Self::Planning | Self::VaultNavigation)
    }
}
