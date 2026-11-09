//! Rust module declarations, `use` paths, `fn main`, and `#[test]`.

use tree_sitter::Node;

use super::{
    field_text, is_module_level, push_entry, push_reference, reference, EntryObservation, Walk,
    ENTRY_PROGRAM, ENTRY_TEST, KIND_IMPORT, KIND_MODULE,
};

pub(super) fn visit(
    node: Node<'_>,
    content: &str,
    references: &mut Vec<super::Reference>,
    entries: &mut Vec<EntryObservation>,
) -> Walk {
    if node.kind() == "mod_item" && node.child_by_field_name("body").is_none() {
        if let Some(name) = field_text(node, "name", content) {
            push_reference(references, reference(KIND_MODULE, name, None, node));
        }
        return Walk::Stop;
    }
    if node.kind() == "use_declaration" {
        rust_uses(node, content, references);
        return Walk::Stop;
    }
    if node.kind() == "function_item" && is_module_level(node) {
        if field_text(node, "name", content) == Some("main") {
            push_entry(
                entries,
                EntryObservation {
                    target_rel_path: None,
                    kind: ENTRY_PROGRAM,
                    reason: "fn main",
                },
            );
        } else if has_test_attribute(node, content) {
            push_entry(
                entries,
                EntryObservation {
                    target_rel_path: None,
                    kind: ENTRY_TEST,
                    reason: "#[test]",
                },
            );
        }
    }
    Walk::Skip
}

fn rust_uses(node: Node<'_>, content: &str, references: &mut Vec<super::Reference>) {
    let Some(argument) = node.child_by_field_name("argument") else {
        return;
    };
    for segments in use_paths(argument, content) {
        let Some(specifier) = segments.last() else {
            continue;
        };
        if segments.len() == 1 && matches!(specifier.as_str(), "crate" | "self" | "super") {
            continue;
        }
        let qualifier = (segments.len() > 1).then(|| segments[..segments.len() - 1].join("::"));
        push_reference(
            references,
            reference(KIND_IMPORT, specifier, qualifier, node),
        );
    }
}

fn use_paths(argument: Node<'_>, content: &str) -> Vec<Vec<String>> {
    fn path_segments(node: Node<'_>, content: &str) -> Vec<String> {
        node.utf8_text(content.as_bytes())
            .unwrap_or_default()
            .split("::")
            .map(str::trim)
            .filter(|segment| !segment.is_empty() && *segment != "*")
            .map(|segment| segment.strip_prefix("r#").unwrap_or(segment).to_string())
            .collect()
    }

    fn collect(node: Node<'_>, prefix: &[String], content: &str, output: &mut Vec<Vec<String>>) {
        match node.kind() {
            "scoped_use_list" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.child_by_field_name("path") {
                    current.extend(path_segments(path, content));
                }
                if let Some(list) = node.child_by_field_name("list") {
                    collect(list, &current, content, output);
                }
            }
            "use_list" => {
                for index in 0..node.named_child_count() {
                    if let Some(child) = node.named_child(index as u32) {
                        collect(child, prefix, content, output);
                    }
                }
            }
            "use_as_clause" => {
                if let Some(path) = node.child_by_field_name("path") {
                    let mut current = prefix.to_vec();
                    current.extend(path_segments(path, content));
                    if !current.is_empty() {
                        output.push(current);
                    }
                }
            }
            "use_wildcard" => {
                let mut current = prefix.to_vec();
                if let Some(path) = node.named_child(0) {
                    current.extend(path_segments(path, content));
                }
                if !current.is_empty() {
                    output.push(current);
                }
            }
            _ => {
                let mut current = prefix.to_vec();
                current.extend(path_segments(node, content));
                if !current.is_empty() {
                    output.push(current);
                }
            }
        }
    }

    let mut output = Vec::new();
    collect(argument, &[], content, &mut output);
    output
}

fn has_test_attribute(node: Node<'_>, content: &str) -> bool {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            break;
        }
        if current
            .utf8_text(content.as_bytes())
            .ok()
            .is_some_and(|text| text.contains("test"))
        {
            return true;
        }
        sibling = current.prev_named_sibling();
    }
    false
}
