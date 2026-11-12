use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

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
                Ok(_) => empty(),
                Err(_) => empty(),
            }
        }
        None => empty(),
    }
}

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
                i += 2;
            }
        } else {
            i += 1;
        }
    }

    Value::Array(links)
}

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
