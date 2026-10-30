//! Metadata filter parser and SQL generation for knowledge search.
//!
//! Parses filter strings like `"type:task status:active"` into SQL WHERE clauses
//! that restrict search results based on enrichment metadata stored in
//! `knowledge_chunk_meta.metadata`.

use anyhow::{anyhow, Result};

/// A parsed filter predicate.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterPredicate {
    /// Filter by source label (joined to knowledge_sources, not metadata).
    Source(String),
    /// Filter by frontmatter field equality:
    /// `json_extract(metadata, '$.frontmatter.<key>') = '<value>'`
    Frontmatter { key: String, value: String },
    /// Filter by tag membership:
    /// `value IN json_each(json_extract(metadata, '$.frontmatter.tags'))`
    Tag(String),
    /// Filter by folder tag:
    /// `value IN json_each(json_extract(metadata, '$.folder_tags'))`
    Folder(String),
    /// Match a source-file extension or an explicit frontmatter language.
    Language(String),
}

/// Validate that a metadata key contains only safe characters for JSON path
/// construction. Allows alphanumeric, underscore, and hyphen.
fn validate_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Parse a filter string into a list of predicates.
///
/// Filter syntax: space-separated `key:value` pairs. Multiple pairs are ANDed.
/// Malformed tokens fail closed. Empty/None input returns an empty vec.
///
/// Special keys:
/// - `source:` — filters by `knowledge_sources.label`
/// - `tag:` — checks membership in `frontmatter.tags` array
/// - `folder:` — checks membership in `folder_tags` array
/// - `language:` — checks the indexed path extension and optional frontmatter
/// - Everything else — treated as a frontmatter field equality check
pub fn parse_filter(filter: Option<&str>) -> Result<Vec<FilterPredicate>> {
    let filter = match filter {
        Some(f) if !f.trim().is_empty() => f.trim(),
        _ => return Ok(vec![]),
    };

    let mut predicates = Vec::new();

    for token in filter.split_whitespace() {
        let (key, value) = token
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid filter token '{token}'; expected key:value"))?;
        if key.is_empty() || value.is_empty() {
            return Err(anyhow!("filter keys and values must not be empty"));
        }
        let pred = match key {
            "source" => FilterPredicate::Source(value.to_string()),
            "tag" => FilterPredicate::Tag(value.to_string()),
            "folder" => FilterPredicate::Folder(value.to_string()),
            "language" => FilterPredicate::Language(value.to_string()),
            other => {
                if !validate_key(other) {
                    return Err(anyhow!("invalid filter key '{other}'"));
                }
                FilterPredicate::Frontmatter {
                    key: other.to_string(),
                    value: value.to_string(),
                }
            }
        };
        predicates.push(pred);
    }

    Ok(predicates)
}

/// Generate SQL WHERE clause fragments and parameter values for a set of predicates.
///
/// Returns `(where_clause, params)` where `where_clause` is a string to append
/// after an existing WHERE (joined with ` AND `), and `params` are the bind values.
///
/// The `param_offset` is the starting `?N` index for parameter placeholders
/// (e.g., if you already have `?1` and `?2` bound, pass `param_offset = 2`).
pub fn predicates_to_sql(
    predicates: &[FilterPredicate],
    param_offset: usize,
) -> (String, Vec<String>) {
    let mut fragments = Vec::new();
    let mut params: Vec<String> = Vec::new();

    for pred in predicates {
        let idx = param_offset + params.len() + 1;
        match pred {
            FilterPredicate::Source(label) => {
                fragments.push(format!(
                    "kcm.source_id = (SELECT id FROM knowledge_sources WHERE label = ?{idx})"
                ));
                params.push(label.clone());
            }
            FilterPredicate::Frontmatter { key, value } => {
                // SQLite json_extract path can't be parameterized, so we build the
                // path string in Rust with a validated key and embed it in the SQL.
                // Only the value is a bound parameter.
                let json_path = format!("$.frontmatter.{key}");
                fragments.push(format!(
                    "json_extract(kcm.metadata, '{json_path}') = ?{idx}"
                ));
                params.push(value.clone());
            }
            FilterPredicate::Tag(tag) => {
                fragments.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(json_extract(kcm.metadata, '$.frontmatter.tags')) WHERE value = ?{idx})"
                ));
                params.push(tag.clone());
            }
            FilterPredicate::Folder(folder) => {
                fragments.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(json_extract(kcm.metadata, '$.folder_tags')) WHERE value = ?{idx})"
                ));
                params.push(folder.clone());
            }
            FilterPredicate::Language(language) => {
                let mut alternatives = Vec::new();
                for extension in crate::language::extensions_for_name(language) {
                    let idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) LIKE ?{idx}"));
                    params.push(format!("%.{}", extension));
                }
                for file_name in crate::language::file_names_for_name(language) {
                    let exact_idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) = ?{exact_idx}"));
                    params.push(file_name.to_string());

                    let suffix_idx = param_offset + params.len() + 1;
                    alternatives.push(format!("lower(kf.rel_path) LIKE ?{suffix_idx}"));
                    params.push(format!("%/{file_name}"));
                }
                if let Some(aliases) = crate::language::aliases_for_name(language) {
                    let placeholders = aliases
                        .iter()
                        .enumerate()
                        .map(|(offset, _)| format!("?{}", param_offset + params.len() + offset + 1))
                        .collect::<Vec<_>>();
                    alternatives.push(format!(
                        "lower(json_extract(kcm.metadata, '$.frontmatter.language')) IN ({})",
                        placeholders.join(", ")
                    ));
                    params.extend(aliases.iter().map(|alias| alias.to_ascii_lowercase()));
                } else {
                    let idx = param_offset + params.len() + 1;
                    alternatives.push(format!(
                        "lower(json_extract(kcm.metadata, '$.frontmatter.language')) = ?{idx}"
                    ));
                    params.push(language.to_ascii_lowercase());
                }
                fragments.push(format!("({})", alternatives.join(" OR ")));
            }
        }
    }

    let where_clause = fragments.join(" AND ");
    (where_clause, params)
}

#[cfg(test)]
mod tests;
