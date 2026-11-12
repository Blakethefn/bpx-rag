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
mod names;
mod python;
mod rust;

pub(super) use names::{is_literal, is_module_level};

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
    pub reason: String,
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

pub(crate) use manifest::implicit_cargo_entries;

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

    if let names::Qualified::Stop(reference) = names::qualified_reference(node, content) {
        if let Some(reference) =
            reference.and_then(|reference| names::normalize_rust(grammar, reference))
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
            names::plain_reference(node, content).and_then(|reference| names::normalize_rust(grammar, reference))
        {
            push_reference(references, reference);
        }
        return false;
    }
    true
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
