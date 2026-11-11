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

/// Valid enrichment names.
const KNOWN_ENRICHMENTS: &[&str] = &["frontmatter", "wikilinks", "folder_tags"];

/// Reject enrichment names this build does not know, so a typo fails at `add`.
pub fn validate_enrichments(names: &[String]) -> anyhow::Result<()> {
    match names
        .iter()
        .find(|name| !KNOWN_ENRICHMENTS.contains(&name.as_str()))
    {
        None => Ok(()),
        Some(name) => Err(anyhow::anyhow!(
            "unknown enrichment '{name}'; choose from: {}",
            KNOWN_ENRICHMENTS.join(", ")
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
) -> Option<Box<dyn Fn(&str, &Path) -> Value>> {
    if enrichments.is_empty() {
        return None;
    }

    let do_frontmatter = enrichments.iter().any(|e| e == "frontmatter");
    let do_wikilinks = enrichments.iter().any(|e| e == "wikilinks");
    let do_folder_tags = enrichments.iter().any(|e| e == "folder_tags");
    let base = source_base_path.to_path_buf();

    // Log unknown enrichment names
    for e in enrichments {
        if !KNOWN_ENRICHMENTS.contains(&e.as_str()) {
            eprintln!("Warning: unknown enrichment '{}', ignoring", e);
        }
    }

    Some(Box::new(move |content: &str, path: &Path| {
        let mut obj = serde_json::Map::new();

        if do_frontmatter {
            let fm = extract_frontmatter(content);
            if !fm.is_null() && fm.as_object().map_or(false, |o| !o.is_empty()) {
                obj.insert("frontmatter".to_string(), fm);
            }
        }
        if do_wikilinks {
            let links = extract_wikilinks(content);
            if let Value::Array(ref arr) = links {
                if !arr.is_empty() {
                    obj.insert("wikilinks".to_string(), links);
                }
            }
        }
        if do_folder_tags {
            let tags = extract_folder_tags(path, &base);
            if let Value::Array(ref arr) = tags {
                if !arr.is_empty() {
                    obj.insert("folder_tags".to_string(), tags);
                }
            }
        }

        Value::Object(obj)
    }))
}

mod extract;
pub use extract::{extract_folder_tags, extract_frontmatter, extract_wikilinks};

#[cfg(test)]
mod tests;
