use tree_sitter::Node;
use super::*;

#[derive(Debug)]
pub(super) struct SyntaxBoundary {
    pub(super) byte: usize,
    pub(super) title: String,
}

pub(super) fn chunk_syntax_tree(
    content: &str,
    root: Node<'_>,
    grammar: crate::relations::Grammar,
) -> Option<Vec<Chunk>> {
    let mut pending = vec![root];
    let mut boundaries = Vec::new();

    while let Some(node) = pending.pop() {
        let parent_is_wrapper = node
            .parent()
            .is_some_and(|parent| is_declaration_wrapper(parent.kind()));
        if is_declaration_boundary(node, grammar, content) && !parent_is_wrapper {
            let byte = include_leading_trivia(node, content);
            boundaries.push(SyntaxBoundary {
                byte,
                title: syntax_title(node, content),
            });
        }

        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }

    boundaries.sort_by(|left, right| left.byte.cmp(&right.byte));
    let mut unique_boundaries: Vec<SyntaxBoundary> = Vec::with_capacity(boundaries.len());
    for boundary in boundaries {
        if boundary.byte > content.len() || !content.is_char_boundary(boundary.byte) {
            continue;
        }
        if unique_boundaries
            .last()
            .is_some_and(|previous| previous.byte == boundary.byte)
        {
            continue;
        }
        unique_boundaries.push(boundary);
    }
    if unique_boundaries.is_empty() {
        return None;
    }

    let mut segments = vec![(0usize, "module".to_string())];
    let mut last_boundary = 0usize;
    for boundary in unique_boundaries {
        if boundary.byte == 0 {
            segments[0].1 = boundary.title;
            continue;
        }
        if boundary.byte.saturating_sub(last_boundary) < MIN_CHUNK_BYTES {
            continue;
        }
        segments.push((boundary.byte, boundary.title));
        last_boundary = boundary.byte;
    }

    let line_starts = line_start_offsets(content);
    let mut chunks = Vec::new();
    for (index, (start, title)) in segments.iter().enumerate() {
        let end = segments
            .get(index + 1)
            .map_or(content.len(), |(next_start, _)| *next_start);
        if end <= *start {
            continue;
        }
        let Some(section) = content.get(*start..end) else {
            return None;
        };
        let line_start = line_for_byte(&line_starts, *start);
        let line_end = line_for_byte(&line_starts, end - 1);
        if section.len() > MAX_CHUNK_BYTES {
            split_at_newlines_with_lines(title, section, &mut chunks, line_start);
        } else {
            chunks.push(Chunk {
                title: title.clone(),
                content: section.to_string(),
                line_start,
                line_end,
            });
        }
    }

    (!chunks.is_empty()).then_some(chunks)
}

pub(super) fn is_declaration_wrapper(kind: &str) -> bool {
    matches!(
        kind,
        "decorated_definition" | "export_statement" | "export_declaration" | "template_declaration"
    )
}

pub(super) fn is_declaration_boundary(
    node: Node<'_>,
    grammar: crate::relations::Grammar,
    content: &str,
) -> bool {
    let kind = node.kind();
    use crate::relations::Grammar;

    match grammar {
        Grammar::Rust => matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "impl_item"
                | "trait_item"
                | "mod_item"
                | "type_item"
                | "const_item"
                | "static_item"
                | "macro_definition"
        ),
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            is_declaration_wrapper(kind)
                || matches!(
                    kind,
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                )
                || (kind == "method_definition"
                    && node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "class_body"))
                || (matches!(kind, "lexical_declaration" | "variable_declaration")
                    && contains_function_value(node))
        }
        Grammar::C | Grammar::Cpp => {
            kind == "function_definition"
                || (matches!(
                    kind,
                    "struct_specifier" | "enum_specifier" | "class_specifier"
                ) && is_type_definition_context(node))
                || (grammar == Grammar::Cpp && kind == "namespace_definition")
        }
        Grammar::CSharp => matches!(
            kind,
            "namespace_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "constructor_declaration"
                | "property_declaration"
                | "delegate_declaration"
        ),
        Grammar::Go => matches!(
            kind,
            "function_declaration" | "method_declaration" | "type_declaration"
        ),
        Grammar::Java => matches!(
            kind,
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "method_declaration"
                | "constructor_declaration"
        ),
        Grammar::Python => matches!(
            kind,
            "decorated_definition" | "function_definition" | "class_definition"
        ),
        Grammar::Php => matches!(
            kind,
            "function_definition"
                | "class_declaration"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "property_declaration"
        ),
        Grammar::Ruby => matches!(kind, "class" | "module" | "method" | "singleton_method"),
        Grammar::Bash => kind == "function_definition",
        Grammar::Lua => kind == "function_declaration",
        Grammar::Dart => matches!(
            kind,
            "class_definition"
                | "enum_declaration"
                | "mixin_declaration"
                | "extension_declaration"
                | "function_declaration"
                | "method_signature"
        ),
        Grammar::Swift => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "enum_declaration"
                | "protocol_declaration"
                | "extension_declaration"
        ),
        Grammar::Kotlin => matches!(
            kind,
            "function_declaration" | "class_declaration" | "object_declaration"
        ),
        Grammar::Sql => kind == "statement",
        Grammar::Scala => matches!(
            kind,
            "class_definition"
                | "trait_definition"
                | "object_definition"
                | "enum_definition"
                | "function_definition"
                | "given_definition"
                | "extension_definition"
                | "package_object"
        ),
        Grammar::Haskell => matches!(
            kind,
            "declaration"
                | "function"
                | "data_type"
                | "newtype"
                | "type_synonym"
                | "class"
                | "instance"
                | "foreign_import"
        ),
        Grammar::Elixir => is_elixir_definition(node, content),
        Grammar::Erlang => matches!(kind, "function_clause" | "ssr_definition"),
        Grammar::R => kind == "function_definition",
        Grammar::Julia => matches!(
            kind,
            "function_definition"
                | "macro_definition"
                | "module_definition"
                | "struct_definition"
                | "abstract_definition"
                | "primitive_definition"
        ),
        Grammar::Perl => matches!(
            kind,
            "package_statement" | "function_definition" | "function_definition_without_sub"
        ),
        Grammar::PowerShell => {
            matches!(
                kind,
                "function_statement" | "class_statement" | "enum_statement"
            )
        }
        Grammar::Solidity => matches!(
            kind,
            "contract_declaration"
                | "interface_declaration"
                | "library_declaration"
                | "function_definition"
                | "constructor_definition"
                | "modifier_definition"
                | "struct_declaration"
                | "enum_declaration"
                | "event_definition"
                | "error_declaration"
                | "user_defined_type_definition"
        ),
        Grammar::Html => {
            matches!(kind, "element" | "script_element" | "style_element")
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "document")
        }
        Grammar::Css => {
            matches!(
                kind,
                "rule_set"
                    | "at_rule"
                    | "charset_statement"
                    | "import_statement"
                    | "keyframes_statement"
                    | "media_statement"
                    | "namespace_statement"
                    | "scope_statement"
                    | "supports_statement"
                    | "declaration"
            ) && node
                .parent()
                .is_some_and(|parent| parent.kind() == "stylesheet")
        }
        Grammar::Markdown => false,
    }
}

pub(super) fn is_elixir_definition(node: Node<'_>, content: &str) -> bool {
    if node.kind() != "call" {
        return false;
    }
    node.child_by_field_name("target")
        .and_then(|target| target.utf8_text(content.as_bytes()).ok())
        .is_some_and(|target| {
            matches!(
                target.trim(),
                "defmodule"
                    | "defprotocol"
                    | "defimpl"
                    | "def"
                    | "defp"
                    | "defmacro"
                    | "defmacrop"
                    | "defguard"
                    | "defguardp"
            )
        })
}

pub(super) fn is_type_definition_context(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matches!(
            parent.kind(),
            "function_definition"
                | "compound_statement"
                | "parameter_declaration"
                | "parameter_list"
        ) {
            return false;
        }
        if matches!(
            parent.kind(),
            "type_definition"
                | "declaration"
                | "field_declaration"
                | "translation_unit"
                | "source_file"
        ) {
            return node.child_by_field_name("body").is_some()
                || (0..node.named_child_count() as u32).any(|index| {
                    node.named_child(index)
                        .is_some_and(|child| child.kind().ends_with("declaration_list"))
                });
        }
        ancestor = parent.parent();
    }
    false
}

pub(super) fn contains_function_value(node: Node<'_>) -> bool {
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if matches!(current.kind(), "arrow_function" | "function_expression") {
            return true;
        }
        for index in (0..current.named_child_count() as u32).rev() {
            if let Some(child) = current.named_child(index) {
                pending.push(child);
            }
        }
    }
    false
}

pub(super) fn include_leading_trivia(node: Node<'_>, content: &str) -> usize {
    let mut start = node.start_byte();
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        if !is_leading_trivia(sibling.kind())
            || sibling.end_byte() > start
            || !starts_on_own_line(content, sibling.start_byte())
        {
            break;
        }
        let Some(gap) = content.get(sibling.end_byte()..start) else {
            break;
        };
        if !gap.chars().all(char::is_whitespace) {
            break;
        }
        start = sibling.start_byte();
        previous = sibling.prev_named_sibling();
    }
    start
}

pub(super) fn starts_on_own_line(content: &str, byte: usize) -> bool {
    let Some(prefix) = content.get(..byte) else {
        return false;
    };
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    content
        .get(line_start..byte)
        .is_some_and(|indentation| indentation.chars().all(char::is_whitespace))
}

pub(super) fn is_leading_trivia(kind: &str) -> bool {
    kind.contains("comment")
        || kind.contains("attribute")
        || matches!(kind, "decorator" | "shebang")
}

pub(super) fn syntax_title(node: Node<'_>, content: &str) -> String {
    let name = declaration_name(node, content);

    let parent_name = node.parent().and_then(|mut parent| loop {
        if is_named_container(parent.kind()) {
            if let Some(name_node) = parent.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                    if !name.trim().is_empty() {
                        return Some(name.trim());
                    }
                }
            }
        }
        parent = parent.parent()?;
    });

    let title = match (parent_name, name) {
        (Some(parent), Some(name)) => format!("{parent}::{name}"),
        (None, Some(name)) => format!("{} {name}", node.kind().replace('_', " ")),
        (Some(parent), None) => format!("{parent}::{}", node.kind().replace('_', " ")),
        (None, None) => node.kind().replace('_', " "),
    };
    truncate_title(&title, 96)
}

pub(super) fn declaration_name<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
            if !name.trim().is_empty() {
                return Some(name.trim());
            }
        }
    }

    let mut pending = Vec::new();
    for index in (0..node.named_child_count() as u32).rev() {
        if let Some(child) = node.named_child(index) {
            pending.push(child);
        }
    }
    while let Some(child) = pending.pop() {
        if let Some(name_node) = child.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                if !name.trim().is_empty() {
                    return Some(name.trim());
                }
            }
        }
        for index in (0..child.named_child_count() as u32).rev() {
            if let Some(grandchild) = child.named_child(index) {
                pending.push(grandchild);
            }
        }
    }
    None
}

pub(super) fn is_named_container(kind: &str) -> bool {
    [
        "class",
        "struct",
        "enum",
        "trait",
        "impl",
        "interface",
        "namespace",
        "module",
        "extension",
        "protocol",
        "object",
        "record",
    ]
    .iter()
    .any(|token| kind.contains(token))
}
