use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

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
