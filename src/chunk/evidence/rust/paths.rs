use tree_sitter::Node;

use super::super::{node_text, push_reference, reference, Reference, KIND_IMPORT};

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

/// `self` at the end of a longer path names the preceding module.
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

pub(super) fn path_segments(node: Node<'_>, content: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    extend_path(node, content, &mut segments)?;
    if node_text(node, content)?.starts_with("::")
        && segments.first().is_none_or(|segment| !segment.is_empty())
    {
        segments.insert(0, String::new());
    }
    if segments.iter().all(String::is_empty) {
        return None;
    }
    Some(segments)
}

fn extend_path(node: Node<'_>, content: &str, segments: &mut Vec<String>) -> Option<()> {
    match node.kind() {
        "scoped_identifier" | "scoped_type_identifier" => {
            let name = node.child_by_field_name("name")?;
            if let Some(path) = node.child_by_field_name("path") {
                extend_path(path, content, segments)?;
            }
            extend_path(name, content, segments)
        }
        "identifier" | "type_identifier" | "crate" | "self" | "super" => {
            segments.push(ident_text(node, content)?);
            Some(())
        }
        _ => None,
    }
}

pub(super) fn join_path(segments: &[String]) -> Option<String> {
    let segments: Vec<String> = segments
        .iter()
        .map(|segment| segment.replace(['"', '\''], ""))
        .collect();
    if segments.is_empty() {
        return None;
    }
    if segments[0].is_empty() {
        let rest = segments[1..].join("::");
        if rest.is_empty() {
            Some("::".to_string())
        } else {
            Some(format!("::{rest}"))
        }
    } else {
        Some(segments.join("::"))
    }
}

fn ident_text(node: Node<'_>, content: &str) -> Option<String> {
    strip_raw(node_text(node, content)?).map(str::to_string)
}

pub(super) fn strip_raw(text: &str) -> Option<&str> {
    let stripped = text.strip_prefix("r#").unwrap_or(text);
    (!stripped.is_empty()).then_some(stripped)
}

