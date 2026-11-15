use tree_sitter::Node;

use super::super::{node_text, push_reference, reference, Reference, KIND_IMPORT};

mod segments;

use segments::ident_text;
pub(super) use segments::{join_path, path_segments, strip_raw};

pub(super) fn collect_use(
    node: Node<'_>,
    prefix: &[String],
    content: &str,
    references: &mut Vec<Reference>,
    anchor: Node<'_>,
) {
    match node.kind() {
        "scoped_use_list" => {
            let mut current = prefix.to_vec();
            if let Some(path) = node.child_by_field_name("path") {
                let Some(segments) = path_segments(path, content) else {
                    return;
                };
                current.extend(segments);
            } else if node_text(node, content).is_some_and(|text| text.starts_with("::")) {
                current.push(String::new());
            }
            if let Some(list) = node.child_by_field_name("list") {
                collect_use(list, &current, content, references, anchor);
            }
        }
        "use_list" => {
            for index in 0..node.named_child_count() {
                if let Some(child) = node.named_child(index as u32) {
                    collect_use(child, prefix, content, references, anchor);
                }
            }
        }
        "use_as_clause" => {
            let Some(path) = node.child_by_field_name("path") else {
                return;
            };
            let Some(alias) = node
                .child_by_field_name("alias")
                .and_then(|alias| ident_text(alias, content))
            else {
                return;
            };
            let Some(segments) = path_segments(path, content) else {
                return;
            };
            let mut full = prefix.to_vec();
            full.extend(segments);
            push_import(references, anchor, &alias, referred_path(&full));
        }
        "use_wildcard" => {
            let mut full = prefix.to_vec();
            if node.named_child_count() > 0 {
                let Some(path) = node.named_child(0) else {
                    return;
                };
                let Some(segments) = path_segments(path, content) else {
                    return;
                };
                full.extend(segments);
            } else if node_text(node, content).is_some_and(|text| text.starts_with("::")) {
                full.push(String::new());
            }
            push_import(references, anchor, "*", referred_path(&full));
        }
        "identifier"
        | "type_identifier"
        | "crate"
        | "self"
        | "super"
        | "scoped_identifier"
        | "scoped_type_identifier" => {
            let Some(segments) = path_segments(node, content) else {
                return;
            };
            let mut full = prefix.to_vec();
            full.extend(segments);
            let referred = referred_path(&full);
            let Some(specifier) = referred.last() else {
                return;
            };
            if specifier.is_empty() {
                return;
            }
            let specifier = specifier.clone();
            push_import(
                references,
                anchor,
                &specifier,
                &referred[..referred.len() - 1],
            );
        }
        _ => {}
    }
}

fn referred_path(segments: &[String]) -> &[String] {
    if segments.len() >= 2 && segments.last().is_some_and(|segment| segment == "self") {
        &segments[..segments.len() - 1]
    } else {
        segments
    }
}

fn push_import(
    references: &mut Vec<Reference>,
    node: Node<'_>,
    specifier: &str,
    qualifier_segments: &[String],
) {
    if specifier.is_empty() {
        return;
    }
    push_reference(
        references,
        reference(KIND_IMPORT, specifier, join_path(qualifier_segments), node),
    );
}
