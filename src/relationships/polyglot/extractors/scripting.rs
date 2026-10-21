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
    let reference = match grammar {
        Grammar::Dart
            if node.kind() == "import_specification" || node.kind() == "library_export" =>
        {
            let Some(uri) = node.child_by_field_name("uri") else {
                return Vec::new();
            };
            let Some(uri_text) = uri.utf8_text(content.as_bytes()).ok().map(str::trim) else {
                return Vec::new();
            };
            let kind = if node.kind() == "library_export" {
                "re-export"
            } else {
                "import"
            };
            quoted_literal(uri_text).map(|specifier| (kind, specifier))
        }
        Grammar::Swift if node.kind() == "import_declaration" => {
            let Some(path) = text
                .strip_prefix("import")
                .map(str::trim)
                .and_then(|path| path.split_whitespace().last())
            else {
                return Vec::new();
            };
            Some(("import", path.to_string()))
        }
        Grammar::Php
            if matches!(
                node.kind(),
                "include_expression"
                    | "require_expression"
                    | "include_once_expression"
                    | "require_once_expression"
            ) =>
        {
            let Some(quote) = text.find(['\'', '"']) else {
                return Vec::new();
            };
            quoted_literal(&text[quote..]).map(|specifier| ("include", specifier))
        }
        Grammar::Ruby
            if node.kind() == "call"
                && (text.starts_with("require ")
                    || text.starts_with("require(")
                    || text.starts_with("require_relative ")
                    || text.starts_with("require_relative(")) =>
        {
            let relative = text.starts_with("require_relative");
            let Some(mut specifier) = quoted_prefix(text) else {
                return Vec::new();
            };
            if relative && !specifier.starts_with('.') && !specifier.starts_with('/') {
                specifier = format!("./{specifier}");
            }
            Some(("import", specifier))
        }
        Grammar::Bash if node.kind() == "command" => {
            let command = text.split_whitespace().collect::<Vec<_>>();
            let operand = match command.as_slice() {
                ["source", operand, ..] | [".", operand, ..] => *operand,
                _ => return Vec::new(),
            };
            let specifier = quoted_literal(operand).unwrap_or_else(|| operand.to_string());
            Some(("include", specifier))
        }
        Grammar::Lua if node.kind() == "function_call" && text.starts_with("require") => {
            let Some(argument) = text
                .strip_prefix("require")
                .map(str::trim)
                .map(|value| value.trim_start_matches('(').trim())
            else {
                return Vec::new();
            };
            quoted_literal(argument).map(|specifier| ("import", specifier))
        }
        _ => None,
    };
    reference.into_iter().collect()
}
