use tree_sitter::Node;

use super::super::{quoted_literal, quoted_prefix};

pub(super) fn ruby_reference(text: &str) -> Option<(&'static str, String)> {
    if !(text.starts_with("require ")
        || text.starts_with("require(")
        || text.starts_with("require_relative ")
        || text.starts_with("require_relative("))
    {
        return None;
    }
    let relative = text.starts_with("require_relative");
    let mut specifier = quoted_prefix(text)?;
    if relative && !specifier.starts_with('.') && !specifier.starts_with('/') {
        specifier = format!("./{specifier}");
    }
    Some(("import", specifier))
}

pub(super) fn bash_reference(text: &str) -> Option<(&'static str, String)> {
    let command = text.split_whitespace().collect::<Vec<_>>();
    let operand = match command.as_slice() {
        ["source", operand, ..] | [".", operand, ..] => *operand,
        _ => return None,
    };
    let specifier = quoted_literal(operand).unwrap_or_else(|| operand.to_string());
    Some(("include", specifier))
}

pub(super) fn lua_reference(text: &str) -> Option<(&'static str, String)> {
    if !text.starts_with("require") {
        return None;
    }
    let argument = text
        .strip_prefix("require")
        .map(str::trim)
        .map(|value| value.trim_start_matches('(').trim())?;
    quoted_literal(argument).map(|specifier| ("import", specifier))
}

pub(super) fn powershell_reference(
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
