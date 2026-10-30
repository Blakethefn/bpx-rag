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
use std::collections::HashSet;
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

/// Extract YAML frontmatter from markdown content.
///
/// Parses the YAML block between `---` delimiters at the start of the file.
/// Returns a JSON object with the parsed fields, or an empty object if:
/// - No frontmatter found
/// - Malformed YAML
/// - YAML parses to a non-object type
pub fn extract_frontmatter(content: &str) -> Value {
    let empty = || Value::Object(serde_json::Map::new());
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return empty();
    }

    let after_first = &trimmed[3..];
    let close_pos = after_first.find("\n---");

    match close_pos {
        Some(pos) => {
            let yaml_str = after_first[..pos].trim();
            if yaml_str.is_empty() {
                return empty();
            }
            match serde_yaml::from_str::<Value>(yaml_str) {
                Ok(val) if val.is_object() => val,
                Ok(_) => empty(),  // Non-object YAML (e.g. bare string, list)
                Err(_) => empty(), // Malformed YAML
            }
        }
        None => empty(), // No closing ---
    }
}

/// Extract wikilinks from content.
///
/// Finds `[[target]]` and `[[target|alias]]` patterns. Returns a JSON array
/// of unique link targets (without aliases). Targets are deduplicated.
pub fn extract_wikilinks(content: &str) -> Value {
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len.saturating_sub(1) {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            if let Some(end_offset) = content[start..].find("]]") {
                let link_content = &content[start..start + end_offset];
                // Extract target (before | if alias exists)
                let target = link_content
                    .split('|')
                    .next()
                    .unwrap_or(link_content)
                    .trim();
                if !target.is_empty() && seen.insert(target.to_string()) {
                    links.push(Value::String(target.to_string()));
                }
                i = start + end_offset + 2;
            } else {
                i += 2; // No closing ]], skip
            }
        } else {
            i += 1;
        }
    }

    Value::Array(links)
}

/// Derive folder tags from a file path relative to the source base.
///
/// Returns a JSON array of directory path components between the source base
/// and the file. The filename itself is excluded.
///
/// Example: file `/vault/01-projects/bpcontext/specs/foo.md` with base `/vault`
/// yields `["01-projects", "bpcontext", "specs"]`.
pub fn extract_folder_tags(file_path: &Path, source_base: &Path) -> Value {
    let rel = match file_path.strip_prefix(source_base) {
        Ok(r) => r,
        Err(_) => return Value::Array(vec![]),
    };

    let tags: Vec<Value> = rel
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .map(|s| Value::String(s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Value::Array(tags)
}

#[cfg(test)]
mod tests;
