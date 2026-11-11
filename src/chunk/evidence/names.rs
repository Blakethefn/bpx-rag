use tree_sitter::Node;

use super::{field_text, node_text, reference, Reference, KIND_NAME, KIND_TYPE};
use crate::relations::Grammar;

/// Rust raw identifiers keep the `r#` prefix in the tree. Stored names omit it.
pub(super) fn normalize_rust(grammar: Grammar, mut reference: Reference) -> Option<Reference> {
    if grammar != Grammar::Rust {
        return Some(reference);
    }
    let specifier = reference
        .specifier
        .strip_prefix("r#")
        .unwrap_or(&reference.specifier)
        .to_string();
    if specifier.is_empty() || is_noise(&specifier) {
        return None;
    }
    reference.specifier = specifier;
    reference.qualifier = reference.qualifier.and_then(|qualifier| {
        let stripped = strip_raw_path(&qualifier);
        (!stripped.is_empty()).then_some(stripped)
    });
    Some(reference)
}

fn strip_raw_path(text: &str) -> String {
    let leading = text.trim_start().starts_with("::");
    let joined = text
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.strip_prefix("r#").unwrap_or(segment))
        .collect::<Vec<_>>()
        .join("::");
    if leading && !joined.is_empty() {
        format!("::{joined}")
    } else {
        joined
    }
}

pub(super) fn plain_reference(node: Node<'_>, content: &str) -> Option<Reference> {
    if is_declaration_name(node) || is_binding_name(node) {
        return None;
    }
    let specifier = node_text(node, content)?;
    if is_noise(specifier) {
        return None;
    }
    let kind = if node.kind() == "type_identifier" {
        KIND_TYPE
    } else {
        KIND_NAME
    };
    Some(reference(kind, specifier, None, node))
}

pub(super) fn qualified_reference(node: Node<'_>, content: &str) -> Option<Reference> {
    let (qualifier_field, name_field, kind) = match node.kind() {
        "scoped_type_identifier" => ("path", "name", KIND_TYPE),
        "scoped_identifier" => ("path", "name", KIND_NAME),
        "attribute" => ("object", "attribute", KIND_NAME),
        "selector_expression" => ("operand", "field", KIND_NAME),
        "member_expression" => ("object", "property", KIND_NAME),
        _ => return None,
    };
    let specifier = field_text(node, name_field, content).or_else(|| {
        node.child_by_field_name(name_field)
            .and_then(|child| node_text(child, content))
    })?;
    if is_noise(specifier) || is_declaration_name(node.child_by_field_name(name_field)?) {
        return None;
    }
    let qualifier = node
        .child_by_field_name(qualifier_field)
        .and_then(|child| node_text(child, content))
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    Some(reference(kind, specifier, qualifier, node))
}

pub(in crate::chunk) fn is_module_level(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matches!(
            parent.kind(),
            "function_item"
                | "function_definition"
                | "function_declaration"
                | "impl_item"
                | "class_definition"
                | "class_body"
                | "method_declaration"
        ) {
            return false;
        }
        ancestor = parent.parent();
    }
    true
}

fn is_declaration_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.child_by_field_name("name") != Some(node) {
        return false;
    }
    matches!(
        parent.kind(),
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "function_signature"
            | "struct_item"
            | "enum_item"
            | "enum_variant"
            | "class_definition"
            | "class_declaration"
            | "method_definition"
            | "method_declaration"
            | "impl_item"
            | "trait_item"
            | "mod_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "variable_declarator"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "class_specifier"
            | "struct_specifier"
    )
}

fn is_binding_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind().contains("parameter") {
        return true;
    }
    matches!(
        parent.kind(),
        "let_declaration" | "let_condition" | "for_expression"
    ) && (parent.child_by_field_name("pattern") == Some(node)
        || parent.child_by_field_name("name") == Some(node))
}

pub(in crate::chunk) fn is_literal(kind: &str) -> bool {
    matches!(
        kind,
        "string"
            | "string_literal"
            | "string_content"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "line_comment"
            | "block_comment"
            | "comment"
    )
}

fn is_noise(specifier: &str) -> bool {
    matches!(
        specifier,
        "self" | "Self" | "super" | "crate" | "cls" | "this" | "_" | "__name__"
    )
}

