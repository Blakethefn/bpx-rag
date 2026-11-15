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
    input: bool,
}

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

}

mod weights;

#[cfg(test)]
mod tests;
