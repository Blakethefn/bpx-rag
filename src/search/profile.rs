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

struct ProfileWire {
    profile: SearchProfile,
    name: &'static str,
    /// Accepted by `parse` and listed on the MCP `profile` enum.
    /// `default` is only emitted by `as_str`.
    input: bool,
}

/// Wire names for search profiles. `parse`, `as_str`, the parse error, and the MCP schema
/// enum all read this table.
const PROFILE_WIRES: &[ProfileWire] = &[
    ProfileWire {
        profile: SearchProfile::Default,
        name: "default",
        input: false,
    },
    ProfileWire {
        profile: SearchProfile::CodeReview,
        name: "code_review",
        input: true,
    },
    ProfileWire {
        profile: SearchProfile::Debug,
        name: "debug",
        input: true,
    },
    ProfileWire {
        profile: SearchProfile::Planning,
        name: "planning",
        input: true,
    },
    ProfileWire {
        profile: SearchProfile::VaultNavigation,
        name: "vault_navigation",
        input: true,
    },
];

impl SearchProfile {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("") => Ok(Self::Default),
            Some(other) => PROFILE_WIRES
                .iter()
                .find(|wire| wire.input && wire.name == other)
                .map(|wire| wire.profile)
                .ok_or_else(|| {
                    anyhow!(
                        "Unknown profile '{other}'. Expected one of: {}",
                        Self::input_names().join(", ")
                    )
                }),
        }
    }

    pub fn as_str(self) -> &'static str {
        PROFILE_WIRES
            .iter()
            .find(|wire| wire.profile == self)
            .map(|wire| wire.name)
            .expect("every search profile has one wire name")
    }

    /// Profile tokens the MCP schema advertises. `default` is not an input.
    pub(crate) fn input_names() -> Vec<&'static str> {
        PROFILE_WIRES
            .iter()
            .filter(|wire| wire.input)
            .map(|wire| wire.name)
            .collect()
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

#[cfg(test)]
mod tests;
