use tree_sitter::Node;

use super::{field_text, node_text, reference, Reference, KIND_NAME, KIND_TYPE};
use crate::relations::Grammar;

mod check;

use check::{is_binding_name, is_declaration_name, is_noise};
pub(in crate::chunk) use check::{is_literal, is_module_level};

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

pub(super) enum Qualified {
    Skip,
    Stop(Option<Reference>),
}

pub(super) fn qualified_reference(node: Node<'_>, content: &str) -> Qualified {
    let (qualifier_field, name_field, kind) = match node.kind() {
        "scoped_type_identifier" => ("path", "name", KIND_TYPE),
        "scoped_identifier" => ("path", "name", KIND_NAME),
        "attribute" => ("object", "attribute", KIND_NAME),
        "selector_expression" => ("operand", "field", KIND_NAME),
        "member_expression" => ("object", "property", KIND_NAME),
        _ => return Qualified::Skip,
    };
    Qualified::Stop(recorded_qualified(
        node,
        content,
        qualifier_field,
        name_field,
        kind,
    ))
}

fn recorded_qualified(
    node: Node<'_>,
    content: &str,
    qualifier_field: &str,
    name_field: &str,
    kind: &'static str,
) -> Option<Reference> {
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
