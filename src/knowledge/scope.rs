//! Which parts of a directory a source covers.
//!
//! A scope is a list of glob patterns. A plain pattern includes matching paths,
//! and a pattern prefixed with `!` excludes them. Scopes are stored as a JSON
//! array in the source's `glob` column. A single plain pattern written before
//! scopes existed is read as a one-element include list.

use anyhow::{Context, Result};
use ignore::overrides::{Override, OverrideBuilder};
use std::path::Path;

/// Combine include and exclude globs into one scope. Excludes get a `!` prefix.
pub fn patterns_from(include: &[String], exclude: &[String]) -> Vec<String> {
    include
        .iter()
        .cloned()
        .chain(exclude.iter().map(|pattern| format!("!{pattern}")))
        .collect()
}

/// Reject patterns the walker cannot compile, so a bad glob fails at `add`.
pub fn validate(base: &Path, patterns: &[String]) -> Result<()> {
    build_overrides(base, patterns).map(|_| ())
}

/// Build walker overrides for a scope. An empty scope covers every file.
pub fn build_overrides(base: &Path, patterns: &[String]) -> Result<Option<Override>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = OverrideBuilder::new(base);
    for pattern in patterns {
        builder
            .add(pattern)
            .with_context(|| format!("invalid source pattern '{pattern}'"))?;
    }
    let overrides = builder.build().context("failed to build source patterns")?;
    Ok(Some(overrides))
}

/// Encode a scope for the `glob` column. An empty scope is stored as NULL.
pub fn encode(patterns: &[String]) -> Option<String> {
    if patterns.is_empty() {
        return None;
    }
    serde_json::to_string(patterns).ok()
}

/// Decode the `glob` column, including the single-pattern form written earlier.
pub fn decode(stored: Option<&str>) -> Vec<String> {
    match stored {
        None => Vec::new(),
        Some(value) if value.starts_with('[') => {
            serde_json::from_str(value).unwrap_or_else(|_| vec![value.to_string()])
        }
        Some(value) => vec![value.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_are_prefixed_and_follow_includes() {
        let patterns = patterns_from(&["src/**".to_string()], &["**/*.test.ts".to_string()]);
        assert_eq!(patterns, vec!["src/**", "!**/*.test.ts"]);
    }

    #[test]
    fn scope_round_trips_through_the_column() {
        let patterns = vec!["src/**".to_string(), "!src/vendor/**".to_string()];
        let stored = encode(&patterns);
        assert_eq!(decode(stored.as_deref()), patterns);
    }

    #[test]
    fn empty_scope_is_stored_as_null() {
        assert_eq!(encode(&[]), None);
        assert!(decode(None).is_empty());
    }

    #[test]
    fn legacy_single_pattern_reads_as_one_include() {
        assert_eq!(decode(Some("docs/**/*.md")), vec!["docs/**/*.md"]);
    }

    #[test]
    fn invalid_pattern_is_rejected_when_validated() {
        let error = validate(Path::new("."), &["[unclosed".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid source pattern '[unclosed'"));
    }
}
