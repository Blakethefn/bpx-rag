//! Which lexical layers run for a query, and why.
//!
//! The trigram and fuzzy layers are gated on candidate-pool size. A policy decides which pool
//! each gate reads. Every layer reports what it did, so intentional skips can be told apart
//! from failures.

use super::lexical::sanitize_trigram_query;
use serde::Serialize;

/// How the trigram and fuzzy layers are gated. The identifier and vector layers are not
/// gated by any policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum LayerPolicy {
    /// Trigram runs while BM25 is below the pool limit. Fuzzy runs while the BM25 and trigram
    /// pool is below the limit.
    #[default]
    CurrentGating,
    /// Trigram and fuzzy each run while BM25 is below the pool limit. Fuzzy ignores trigram's
    /// additions.
    OwnPoolGating,
    /// Trigram and fuzzy always run, subject only to their own input checks.
    Unconditional,
}

/// Why a layer did not run. Each reason is intentional, so none of them is a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The layer's weight is zero.
    Disabled,
    /// The layer's pool gate was reached, so the layer was not run.
    PoolFull,
    /// The query is shorter than the trigram minimum of three bytes.
    QueryTooShort,
    /// Nothing in the query survives sanitizing.
    NoQueryTerms,
    /// Fuzzy found no query word to correct.
    NoCorrectedWord,
    /// The query has fewer than two content words, so identifier matching is not attempted.
    TooFewContentWords,
    /// No embedder is attached to the store.
    NoEmbedder,
}

impl SkipReason {
    /// The short label shown in `explain` output.
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

/// The outcome of one layer for one query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LayerOutcome {
    /// The layer ran and returned candidates.
    Ran,
    /// The layer did not run, for an intentional reason.
    Skipped { reason: SkipReason },
    /// The layer ran into an error that did not stop the search.
    Failed { error: String },
}

/// One layer's diagnostics for one search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerReport {
    pub layer: &'static str,
    pub outcome: LayerOutcome,
    /// Chunks the layer returned before fusion. Zero when the layer did not run.
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

/// The trigram layer needs at least this many bytes in the query.
const TRIGRAM_MIN_BYTES: usize = 3;

/// Whether trigram should be skipped. `None` means it runs. The query checks apply under
/// every policy. The pool check depends on the policy.
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

/// Whether fuzzy should be skipped. `None` means it runs. The current policy counts the
/// BM25 and trigram pool. The own-pool policy counts only the BM25 pool. Identifier results
/// are never counted.
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
