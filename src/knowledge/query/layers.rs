use super::lexical::sanitize_trigram_query;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum LayerPolicy {
    #[value(
        help = "Trigram runs while BM25 is below the pool limit. Fuzzy runs while the BM25 and trigram pool is below the limit"
    )]
    #[default]
    CurrentGating,
    #[value(
        help = "Trigram and fuzzy each run while BM25 is below the pool limit. Fuzzy ignores trigram's additions"
    )]
    OwnPoolGating,
    #[value(help = "Trigram and fuzzy always run, subject only to their own input checks")]
    Unconditional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Disabled,
    PoolFull,
    QueryTooShort,
    NoQueryTerms,
    NoCorrectedWord,
    TooFewContentWords,
    NoEmbedder,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "weight is zero",
            Self::PoolFull => "candidate pool full",
            Self::QueryTooShort => "query under three bytes",
            Self::NoQueryTerms => "no query terms",
            Self::NoCorrectedWord => "no corrected word",
            Self::TooFewContentWords => "fewer than two content words",
            Self::NoEmbedder => "no embedder attached",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LayerOutcome {
    Ran,
    Skipped { reason: SkipReason },
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerReport {
    pub layer: &'static str,
    pub outcome: LayerOutcome,
    pub candidates: usize,
}

impl LayerReport {
    pub(super) fn ran(layer: &'static str, candidates: usize) -> Self {
        Self {
            layer,
            outcome: LayerOutcome::Ran,
            candidates,
        }
    }

    pub(super) fn skipped(layer: &'static str, reason: SkipReason) -> Self {
        Self {
            layer,
            outcome: LayerOutcome::Skipped { reason },
            candidates: 0,
        }
    }

    pub(super) fn failed(layer: &'static str, error: String) -> Self {
        Self {
            layer,
            outcome: LayerOutcome::Failed { error },
            candidates: 0,
        }
    }
}

const TRIGRAM_MIN_BYTES: usize = 3;

pub(super) fn trigram_skip(
    policy: LayerPolicy,
    query: &str,
    bm25_len: usize,
    candidate_limit: usize,
) -> Option<SkipReason> {
    if query.len() < TRIGRAM_MIN_BYTES {
        return Some(SkipReason::QueryTooShort);
    }
    if sanitize_trigram_query(query).is_empty() {
        return Some(SkipReason::NoQueryTerms);
    }
    match policy {
        LayerPolicy::CurrentGating | LayerPolicy::OwnPoolGating => {
            (bm25_len >= candidate_limit).then_some(SkipReason::PoolFull)
        }
        LayerPolicy::Unconditional => None,
    }
}

pub(super) fn fuzzy_skip(
    policy: LayerPolicy,
    bm25_len: usize,
    lexical_pool_len: usize,
    candidate_limit: usize,
) -> Option<SkipReason> {
    let pool_len = match policy {
        LayerPolicy::CurrentGating => lexical_pool_len,
        LayerPolicy::OwnPoolGating => bm25_len,
        LayerPolicy::Unconditional => return None,
    };
    (pool_len >= candidate_limit).then_some(SkipReason::PoolFull)
}

#[cfg(test)]
mod tests;
