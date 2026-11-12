use anyhow::{Context, Result};
use ignore::overrides::{Override, OverrideBuilder};
use std::path::Path;

pub fn patterns_from(include: &[String], exclude: &[String]) -> Vec<String> {
    include
        .iter()
        .cloned()
        .chain(exclude.iter().map(|pattern| format!("!{pattern}")))
        .collect()
}

pub fn validate(base: &Path, patterns: &[String]) -> Result<()> {
    build_overrides(base, patterns).map(|_| ())
}

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

pub fn encode(patterns: &[String]) -> Option<String> {
    if patterns.is_empty() {
        return None;
    }
    serde_json::to_string(patterns).ok()
}

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
