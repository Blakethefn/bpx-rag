use tree_sitter::Node;

use super::super::super::super::languages::Grammar;
use super::{quoted_literal, quoted_prefix};

pub(super) fn extract(
    grammar: Grammar,
    node: Node<'_>,
    content: &str,
) -> Vec<(&'static str, String)> {
    let Some(text) = node.utf8_text(content.as_bytes()).ok().map(str::trim) else {
        return Vec::new();
    };
    let specifier = match grammar {
        Grammar::Html if node.kind() == "attribute" => {
            let Some(name) = node.named_child(0) else {
                return Vec::new();
            };
            let Some(name) = name.utf8_text(content.as_bytes()).ok() else {
                return Vec::new();
            };
            if !matches!(name.to_ascii_lowercase().as_str(), "src" | "href") {
                return Vec::new();
            }
            let Some(value) = node.named_child(1) else {
                return Vec::new();
            };
            let Some(value) = value.utf8_text(content.as_bytes()).ok() else {
                return Vec::new();
            };
            quoted_literal(value).unwrap_or_else(|| value.trim().to_string())
        }
        Grammar::Css if node.kind() == "import_statement" => {
            let Some(path) = quoted_prefix(text).or_else(|| css_import_url(text)) else {
                return Vec::new();
            };
            path
        }
        _ => return Vec::new(),
    };
    let specifier = local_path(&specifier);
    specifier
        .filter(|path| !path.is_empty())
        .map(|path| vec![("include", path)])
        .unwrap_or_default()
}

fn css_import_url(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("url(")? + "url(".len();
    let value = text[start..].trim_start();
    let end = value.find(')')?;
    let value = value[..end].trim();
    Some(quoted_literal(value).unwrap_or_else(|| value.to_string()))
}

fn local_path(specifier: &str) -> Option<String> {
    let value = specifier.trim();
    let lower = value.to_ascii_lowercase();
    if value.is_empty()
        || value.starts_with('#')
        || value.starts_with("//")
        || lower.contains("://")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
        || lower.starts_with("javascript:")
    {
        return None;
    }
    Some(
        value
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .replace('\\', "/"),
    )
}
