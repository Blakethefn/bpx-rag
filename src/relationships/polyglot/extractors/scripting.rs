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
        Grammar::Elixir => elixir_reference(node, content),
        Grammar::R => r_reference(node, content),
        Grammar::Julia => julia_reference(node, text, content),
        Grammar::Perl => perl_reference(node, content),
        Grammar::PowerShell => powershell_reference(node, text, content),
        _ => None,
    };
    reference.into_iter().collect()
}

fn elixir_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    if node.kind() != "call" {
        return None;
    }
    let target = node
        .child_by_field_name("target")?
        .utf8_text(content.as_bytes())
        .ok()?
        .trim();
    if !matches!(target, "alias" | "import" | "require" | "use") {
        return None;
    }
    let arguments = node
        .named_child(1)?
        .utf8_text(content.as_bytes())
        .ok()?
        .trim()
        .trim_start_matches('(');
    let module = arguments
        .split([')', ',', '{'])
        .next()?
        .trim()
        .trim_end_matches(" do");
    (!module.is_empty()).then(|| ("import", module.to_string()))
}

fn r_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    if node.kind() != "call" {
        return None;
    }
    let function = node
        .child_by_field_name("function")?
        .utf8_text(content.as_bytes())
        .ok()?
        .trim();
    if !matches!(function, "source" | "sys.source") {
        return None;
    }
    let arguments = node
        .child_by_field_name("arguments")?
        .utf8_text(content.as_bytes())
        .ok()?;
    quoted_prefix(arguments).map(|specifier| ("include", specifier))
}

fn julia_reference(node: Node<'_>, text: &str, content: &str) -> Option<(&'static str, String)> {
    if matches!(node.kind(), "import_statement" | "using_statement") {
        let keyword = if node.kind() == "using_statement" {
            "using"
        } else {
            "import"
        };
        let module = text.strip_prefix(keyword)?.trim().trim_end_matches(';');
        return (!module.is_empty()).then(|| ("import", module.to_string()));
    }
    if node.kind() != "call_expression" || !text.trim_start().starts_with("include") {
        return None;
    }
    let arguments = node
        .named_child(1)
        .and_then(|child| child.utf8_text(content.as_bytes()).ok())
        .unwrap_or(text);
    quoted_prefix(arguments).map(|specifier| ("include", specifier))
}

fn perl_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
    match node.kind() {
        "use_no_statement" if node.child_by_field_name("use").is_some() => node
            .child_by_field_name("package_name")
            .and_then(|package| package.utf8_text(content.as_bytes()).ok())
            .map(str::trim)
            .filter(|package| !package.is_empty())
            .map(|package| ("import", package.to_string())),
        "require_statement" => node
            .child_by_field_name("package_name")
            .and_then(|package| package.utf8_text(content.as_bytes()).ok())
            .and_then(|package| {
                quoted_literal(package).or_else(|| Some(package.trim().to_string()))
            })
            .filter(|package| !package.is_empty())
            .map(|package| ("include", package)),
        _ => None,
    }
}

fn powershell_reference(
    node: Node<'_>,
    text: &str,
    content: &str,
) -> Option<(&'static str, String)> {
    if node.kind() == "using_statement" {
        let trimmed = text.trim_start();
        if trimmed.to_ascii_lowercase().starts_with("using module ") {
            let remainder = trimmed.get("using module".len()..)?.trim();
            return first_path_argument(remainder).map(|path| ("import", path));
        }
        return None;
    }
    if node.kind() != "command" {
        return None;
    }
    let command_name = node
        .child_by_field_name("command_name")
        .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        .map(str::trim)
        .unwrap_or_default();
    let dot_source = (0..node.child_count() as u32).any(|index| {
        node.child(index)
            .is_some_and(|child| child.kind() == "command_invokation_operator")
    }) && node
        .child_by_field_name("command_name")
        .is_some_and(|name| name.kind() == "command_name_expr");
    if dot_source {
        return first_path_argument(command_name).map(|path| ("include", path));
    }
    let command = node
        .child_by_field_name("command_name")
        .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        .map(str::trim)
        .unwrap_or_default();
    let remainder = text.strip_prefix(command)?.trim();
    if command.eq_ignore_ascii_case("Import-Module") {
        return first_path_argument(remainder).map(|path| ("import", path));
    }
    None
}

fn first_path_argument(arguments: &str) -> Option<String> {
    let arguments = arguments.trim_start();
    let argument = if let Some(quoted) = quoted_prefix(arguments) {
        quoted
    } else {
        arguments
            .split_whitespace()
            .find(|part| !part.starts_with('-'))?
            .trim_end_matches([';', ','])
            .to_string()
    };
    (!argument.is_empty()).then_some(argument)
}
