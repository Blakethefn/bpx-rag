//! Enrichment pipeline for knowledge store metadata extraction.
//!
//! Enrichments are optional metadata extractors that run during sync to produce
//! structured key-value pairs stored alongside each chunk. They enable filtered
//! search — e.g., "find all active task notes about the prediction engine".
//!
//! Three built-in enrichments:
//! - **frontmatter** — parses YAML between `---` fences at file start
//! - **wikilinks** — extracts `[[target]]` and `[[target|alias]]` patterns
//! - **folder_tags** — derives tags from directory path components

use serde_json::Value;
use std::path::Path;

/// Extractor for one enrichment. `None` means the value is empty and is not stored.
type Extract = fn(&str, &Path, &Path) -> Option<Value>;

/// The only catalog. Each entry pairs a name with its extractor; that name is the JSON key.
const CATALOG: &[(&str, Extract)] = &[
    ("frontmatter", frontmatter_value),
    ("wikilinks", wikilinks_value),
    ("folder_tags", folder_tags_value),
];

/// Names in `CATALOG` order. Keep the two tables in sync.
const NAMES: &[&str] = &["frontmatter", "wikilinks", "folder_tags"];

fn enrichment(name: &str) -> Option<(&'static str, Extract)> {
    CATALOG.iter().find(|(key, _)| *key == name).copied()
}

fn enrichment_names() -> &'static [&'static str] {
    NAMES
}

/// Boxed enrichment closure returned by `build_enrichment_fn`.
type BoxedEnrichment = Box<dyn Fn(&str, &Path) -> Value>;

fn frontmatter_value(content: &str, _path: &Path, _base: &Path) -> Option<Value> {
    let value = extract_frontmatter(content);
    if value.as_object().is_some_and(|fields| !fields.is_empty()) {
        Some(value)
    } else {
        None
    }
}

fn wikilinks_value(content: &str, _path: &Path, _base: &Path) -> Option<Value> {
    present_array(extract_wikilinks(content))
}

fn folder_tags_value(_content: &str, path: &Path, base: &Path) -> Option<Value> {
    present_array(extract_folder_tags(path, base))
}

fn present_array(value: Value) -> Option<Value> {
    if value.as_array().is_some_and(|items| !items.is_empty()) {
        Some(value)
    } else {
        None
    }
}

/// Reject enrichment names this build does not know, so a typo fails at `add`.
pub fn validate_enrichments(names: &[String]) -> anyhow::Result<()> {
    match names.iter().find(|name| enrichment(name).is_none()) {
        None => Ok(()),
        Some(name) => Err(anyhow::anyhow!(
            "unknown enrichment '{name}'; choose from: {}",
            enrichment_names().join(", ")
        )),
    }
}

/// Build an enrichment function from a list of enabled enrichment names.
/// Returns `None` if the list is empty.
///
/// Valid names: `"frontmatter"`, `"wikilinks"`, `"folder_tags"`.
/// Unknown names are logged as warnings and ignored.
pub fn build_enrichment_fn(
    enrichments: &[String],
    source_base_path: &Path,
) -> Option<BoxedEnrichment> {
    if enrichments.is_empty() {
        return None;
    }

    let mut selected = Vec::new();
    for name in enrichments {
        match enrichment(name) {
            Some(spec) => selected.push(spec),
            None => eprintln!("Warning: unknown enrichment '{name}', ignoring"),
        }
    }
    let base = source_base_path.to_path_buf();

    Some(Box::new(move |content: &str, path: &Path| {
        let mut obj = serde_json::Map::new();
        for (name, extract) in &selected {
            if let Some(value) = extract(content, path, &base) {
                obj.insert((*name).to_string(), value);
            }
        }
        Value::Object(obj)
    }))
}

mod extract;
pub use extract::{extract_folder_tags, extract_frontmatter, extract_wikilinks};

#[cfg(test)]
mod tests;
