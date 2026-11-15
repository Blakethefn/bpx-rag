use super::SearchProfile;

impl SearchProfile {
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
