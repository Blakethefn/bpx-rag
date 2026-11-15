mod import;
mod shell;

use tree_sitter::Node;

use super::{has_kind, quoted_literal};

pub(super) fn extract(node: Node<'_>, content: &str) -> Vec<(&'static str, String)> {
    let Some(text) = node.utf8_text(content.as_bytes()).ok().map(str::trim) else {
        return Vec::new();
    };
    let reference = match node.kind() {
        "import_specification" | "library_export" => {
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
        "import_declaration" => {
            let Some(path) = text
                .strip_prefix("import")
                .map(str::trim)
                .and_then(|path| path.split_whitespace().last())
            else {
                return Vec::new();
            };
            Some(("import", path.to_string()))
        }
        "include_expression"
        | "require_expression"
        | "include_once_expression"
        | "require_once_expression" => {
            let Some(quote) = text.find(['\'', '"']) else {
                return Vec::new();
            };
            quoted_literal(&text[quote..]).map(|specifier| ("include", specifier))
        }
        "call" => call_reference(node, text, content),
        "command" => command_reference(node, text, content),
        "function_call" => shell::lua_reference(text),
        "import_statement" => import::julia_reference(node, text, content),
        "using_statement" => shell::powershell_reference(node, text, content)
            .or_else(|| import::julia_reference(node, text, content)),
        "call_expression" => {
            if node.child_by_field_name("function").is_some() || has_kind(node, "call_suffix") {
                None
            } else {
                import::julia_reference(node, text, content)
            }
        }
        "use_no_statement" | "require_statement" => import::perl_reference(node, content),
        _ => None,
    };
    reference.into_iter().collect()
}

fn call_reference(node: Node<'_>, text: &str, content: &str) -> Option<(&'static str, String)> {
    if node.child_by_field_name("target").is_some() {
        return import::elixir_reference(node, content);
    }
    if node.child_by_field_name("function").is_some() {
        return import::r_reference(node, content);
    }
    shell::ruby_reference(text)
}

fn command_reference(node: Node<'_>, text: &str, content: &str) -> Option<(&'static str, String)> {
    if node.child_by_field_name("command_name").is_some() {
        return shell::powershell_reference(node, text, content);
    }
    shell::bash_reference(text)
}

