//! Rust module declarations, `use` paths, `fn main`, and `#[test]`.

use tree_sitter::Node;

use super::{
    field_text, is_module_level, node_text, push_entry, push_reference, reference,
    EntryObservation, Walk, ENTRY_PROGRAM, ENTRY_TEST, KIND_MODULE,
};

mod attributes;
mod paths;

use attributes::{has_test_attribute, mod_path};
use paths::{collect_use, join_path, path_segments, strip_raw};

pub(super) fn visit(
    node: Node<'_>,
    content: &str,
    references: &mut Vec<super::Reference>,
    entries: &mut Vec<EntryObservation>,
) -> Walk {
    if node.kind() == "mod_item" && node.child_by_field_name("body").is_none() {
        if let Some(name) = field_text(node, "name", content).and_then(strip_raw) {
            push_reference(
                references,
                reference(KIND_MODULE, name, mod_path(node, content), node),
            );
        }
        return Walk::Stop;
    }
    if node.kind() == "use_declaration" {
        if let Some(argument) = node.child_by_field_name("argument") {
            collect_use(argument, &[], content, references, node);
        }
        return Walk::Stop;
    }
    if node.kind() == "function_item" && is_module_level(node) {
        if field_text(node, "name", content) == Some("main") {
            push_entry(
                entries,
                EntryObservation {
                    target_rel_path: None,
                    kind: ENTRY_PROGRAM,
                    reason: "fn main".to_string(),
                },
            );
        } else if has_test_attribute(node, content) {
            if let Some(name) = field_text(node, "name", content) {
                push_entry(
                    entries,
                    EntryObservation {
                        target_rel_path: None,
                        kind: ENTRY_TEST,
                        reason: format!("#[test] {name}"),
                    },
                );
            }
        }
    }
    Walk::Skip
}

/// Visibility written on a Rust declaration. `None` means a modifier was present and could not
/// be read. Callers store `None` for every other language.
pub(super) fn item_visibility(node: Node<'_>, content: &str) -> Option<String> {
    let modifier = (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .find(|child| child.kind() == "visibility_modifier");
    let Some(modifier) = modifier else {
        return Some("private".to_string());
    };
    format_visibility(modifier, content)
}

fn format_visibility(node: Node<'_>, content: &str) -> Option<String> {
    if node.named_child_count() > 1 {
        return None;
    }
    let text = node_text(node, content)?;
    if !text.starts_with("pub") {
        return None;
    }
    let Some(child) = node.named_child(0) else {
        return (text == "pub").then(|| "pub".to_string());
    };
    if has_anonymous_token(node, content, "in") {
        let path = join_path(&path_segments(child, content)?)?;
        return Some(format!("pub(in {path})"));
    }
    match child.kind() {
        "crate" => Some("pub(crate)".to_string()),
        "super" => Some("pub(super)".to_string()),
        "self" => Some("pub(self)".to_string()),
        _ => None,
    }
}

fn has_anonymous_token(node: Node<'_>, content: &str, token: &str) -> bool {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return false;
    }
    loop {
        let child = cursor.node();
        if !child.is_named() && node_text(child, content) == Some(token) {
            return true;
        }
        if !cursor.goto_next_sibling() {
            return false;
        }
    }
}
