use tree_sitter::Node;

use super::super::{quoted_literal, quoted_prefix};

pub(super) fn elixir_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
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

pub(super) fn r_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
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

pub(super) fn julia_reference(node: Node<'_>, text: &str, content: &str) -> Option<(&'static str, String)> {
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

pub(super) fn perl_reference(node: Node<'_>, content: &str) -> Option<(&'static str, String)> {
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
