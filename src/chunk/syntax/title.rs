use super::super::truncate_title;
use tree_sitter::Node;

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

pub(in crate::chunk) fn is_named_container(kind: &str) -> bool {
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
