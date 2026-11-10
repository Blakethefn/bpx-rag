//! Symbol extraction from the Tree-sitter tree used for chunking.
//!
//! A symbol is a named declaration boundary from the chunker's own walk. The same nodes become
//! chunk titles, so every symbol sits inside a chunk. The extractor reads the tree that chunking
//! already built, so it costs no extra parse for a changed file.

use super::syntax::{declaration_nodes, is_declaration_wrapper, is_named_container};
use crate::relations::Grammar;
#[cfg(test)]
use std::path::Path;
use tree_sitter::Node;

/// A named declaration in a source file. Lines are 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// The node kind with its grammar suffix removed, such as `function` for `function_item`.
    pub kind: String,
    /// The nearest enclosing named container, such as an impl type or a class.
    pub container: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    /// Rust visibility: `private`, `pub`, `pub(crate)`, `pub(super)`, `pub(self)`, or
    /// `pub(in <path>)`. `None` for other languages, and for a Rust modifier that could not be read.
    pub visibility: Option<String>,
}

/// Symbols for one file. Empty for an unsupported language, a parse error, a file above the
/// Tree-sitter cap, or a finished walk that declared nothing. The status on
/// [`super::chunk_content_for_path`] distinguishes those cases.
#[cfg(test)]
pub(crate) fn symbols_for_path(path: &Path, content: &str) -> Vec<Symbol> {
    super::chunk_content_for_path(path, content).symbols
}

/// Symbols from an already-parsed tree. The caller decides whether the tree is usable.
pub(super) fn symbols_from_tree(content: &str, root: Node<'_>, grammar: Grammar) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for node in declaration_nodes(root, content, grammar) {
        collect(node, content, grammar, &mut symbols);
    }
    symbols
}

fn collect(node: Node<'_>, content: &str, grammar: Grammar, out: &mut Vec<Symbol>) {
    if let Some(inner) = wrapped_declaration(node) {
        return collect(inner, content, grammar, out);
    }
    match node.kind() {
        "type_declaration" => {
            for spec in named_children(node) {
                if matches!(spec.kind(), "type_spec" | "type_alias") {
                    push_named(spec, go_type_kind(spec), content, grammar, out);
                }
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            for declarator in named_children(node) {
                if declarator.kind() == "variable_declarator" {
                    push_named(declarator, "variable".to_string(), content, grammar, out);
                }
            }
        }
        kind => push_named(node, declaration_kind(kind), content, grammar, out),
    }
}

/// The declaration a wrapper such as `export` or a Python decorator stands for.
fn wrapped_declaration(node: Node<'_>) -> Option<Node<'_>> {
    if !is_declaration_wrapper(node.kind()) {
        return None;
    }
    node.child_by_field_name("declaration")
        .or_else(|| node.child_by_field_name("definition"))
}

fn push_named(
    node: Node<'_>,
    kind: String,
    content: &str,
    grammar: Grammar,
    out: &mut Vec<Symbol>,
) {
    let Some(name) = field_text(node, "name", content) else {
        return;
    };
    out.push(Symbol {
        name: name.to_string(),
        kind,
        container: container_name(node, content),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: (grammar == Grammar::Rust)
            .then(|| super::evidence::rust_item_visibility(node, content))
            .flatten(),
    });
}

/// Strip one grammar suffix from a node kind: `function_item` becomes `function`.
fn declaration_kind(kind: &str) -> String {
    const SUFFIXES: [&str; 5] = [
        "_item",
        "_definition",
        "_declaration",
        "_specifier",
        "_statement",
    ];
    SUFFIXES
        .iter()
        .find_map(|suffix| kind.strip_suffix(suffix))
        .unwrap_or(kind)
        .to_string()
}

fn go_type_kind(spec: Node<'_>) -> String {
    match spec
        .child_by_field_name("type")
        .map(|type_node| type_node.kind())
    {
        Some("struct_type") => "struct",
        Some("interface_type") => "interface",
        _ => "type",
    }
    .to_string()
}

/// The nearest enclosing container with a name. An `impl` names itself through its `type` field.
fn container_name(node: Node<'_>, content: &str) -> Option<String> {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if is_symbol_container(parent.kind()) {
            if let Some(name) =
                field_text(parent, "name", content).or_else(|| field_text(parent, "type", content))
            {
                return Some(name.to_string());
            }
        }
        ancestor = parent.parent();
    }
    None
}

fn is_symbol_container(kind: &str) -> bool {
    is_named_container(kind) || kind == "mod_item"
}

fn field_text<'a>(node: Node<'_>, field: &str, content: &'a str) -> Option<&'a str> {
    let text = node
        .child_by_field_name(field)?
        .utf8_text(content.as_bytes())
        .ok()?
        .trim();
    (!text.is_empty()).then_some(text)
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count() as u32)
        .filter_map(|index| node.named_child(index))
        .collect()
}

#[cfg(test)]
mod tests;
