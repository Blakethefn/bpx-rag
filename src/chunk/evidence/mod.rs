//! Occurrences and entry observations taken from one file's bytes.
//!
//! A reference records what was written. It does not name a target. Callers store the rows and
//! resolve them later from the indexed symbols and files.
//!
//! Each language owns the imports and entry points that grammar writes. This module owns the
//! shared name walk and the types those files record.

mod go;
mod javascript;
mod manifest;
mod python;
mod rust;

use std::path::Path;

use tree_sitter::Node;

use crate::relations::Grammar;

pub(crate) const STATUS_COMPLETE: &str = "complete";
pub(crate) const STATUS_UNSUPPORTED: &str = "unsupported";
pub(crate) const STATUS_PARSE_ERROR: &str = "parse_error";
pub(crate) const STATUS_TOO_LARGE: &str = "too_large";

pub(crate) const KIND_NAME: &str = "name";
pub(crate) const KIND_IMPORT: &str = "import";
pub(crate) const KIND_MODULE: &str = "module";
pub(crate) const KIND_PATH: &str = "path";
pub(crate) const KIND_TYPE: &str = "type";

pub(crate) const ENTRY_PROGRAM: &str = "program";
pub(crate) const ENTRY_TEST: &str = "test";
pub(crate) const ENTRY_LIBRARY: &str = "library_root";

/// One occurrence in the file that contains it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    pub kind: &'static str,
    pub specifier: String,
    pub qualifier: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
}

/// A program, test, or library-root observation. `target_rel_path` is source-relative.
/// `None` means the file that contains the observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryObservation {
    pub target_rel_path: Option<String>,
    pub kind: &'static str,
    pub reason: &'static str,
}

/// `Stop` means the language claimed the node and its children are not walked again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Walk {
    Skip,
    Stop,
}

pub(super) fn references_and_entries(
    path: &Path,
    content: &str,
    root: Node<'_>,
    grammar: Grammar,
) -> (Vec<Reference>, Vec<EntryObservation>) {
    let mut references = Vec::new();
    let mut entries = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if !visit(grammar, path, node, content, &mut references, &mut entries) {
            continue;
        }
        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    (references, entries)
}

pub(super) fn manifest_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
    manifest::manifest_entries(path, content)
}

/// Returns whether the walk should enter the node's children.
fn visit(
    grammar: Grammar,
    path: &Path,
    node: Node<'_>,
    content: &str,
    references: &mut Vec<Reference>,
    entries: &mut Vec<EntryObservation>,
) -> bool {
    if is_literal(node.kind()) {
        return false;
    }
    let claimed = match grammar {
        Grammar::Rust => rust::visit(node, content, references, entries),
        Grammar::Python => python::visit(node, content, references, entries),
        Grammar::Go => go::visit(path, node, content, references, entries),
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            javascript::visit(node, content, references)
        }
        _ => Walk::Skip,
    };
    if claimed == Walk::Stop {
        return false;
    }

    if matches!(
        node.kind(),
        "scoped_identifier"
            | "scoped_type_identifier"
            | "attribute"
            | "selector_expression"
            | "member_expression"
    ) {
        if let Some(reference) = qualified_reference(node, content)
            .and_then(|reference| normalize_rust(grammar, reference))
        {
            push_reference(references, reference);
        }
        return false;
    }

    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier" | "property_identifier"
    ) {
        if let Some(reference) =
            plain_reference(node, content).and_then(|reference| normalize_rust(grammar, reference))
        {
            push_reference(references, reference);
        }
        return false;
    }
    true
}

/// Rust raw identifiers keep the `r#` prefix in the tree. Stored names omit it.
fn normalize_rust(grammar: Grammar, mut reference: Reference) -> Option<Reference> {
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

fn plain_reference(node: Node<'_>, content: &str) -> Option<Reference> {
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

fn qualified_reference(node: Node<'_>, content: &str) -> Option<Reference> {
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

pub(super) fn reference(
    kind: &'static str,
    specifier: &str,
    qualifier: Option<String>,
    node: Node<'_>,
) -> Reference {
    Reference {
        kind,
        specifier: specifier.to_string(),
        qualifier,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
    }
}

pub(super) fn push_reference(references: &mut Vec<Reference>, reference: Reference) {
    if references.iter().any(|existing| existing == &reference) {
        return;
    }
    references.push(reference);
}

pub(super) fn push_entry(entries: &mut Vec<EntryObservation>, entry: EntryObservation) {
    if entries.iter().any(|existing| {
        existing.kind == entry.kind
            && existing.reason == entry.reason
            && existing.target_rel_path == entry.target_rel_path
    }) {
        return;
    }
    entries.push(entry);
}

pub(super) fn is_module_level(node: Node<'_>) -> bool {
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

pub(super) fn is_literal(kind: &str) -> bool {
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

pub(super) fn field_text<'a>(node: Node<'_>, field: &str, content: &'a str) -> Option<&'a str> {
    node_text(node.child_by_field_name(field)?, content)
}

pub(super) fn rust_item_visibility(node: Node<'_>, content: &str) -> Option<String> {
    rust::item_visibility(node, content)
}

pub(super) fn node_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests;
