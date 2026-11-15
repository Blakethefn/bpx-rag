use tree_sitter::Node;

use super::super::super::node_text;

pub(in crate::chunk::evidence::rust) fn path_segments(
    node: Node<'_>,
    content: &str,
) -> Option<Vec<String>> {
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

pub(in crate::chunk::evidence::rust) fn join_path(segments: &[String]) -> Option<String> {
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

pub(super) fn ident_text(node: Node<'_>, content: &str) -> Option<String> {
    strip_raw(node_text(node, content)?).map(str::to_string)
}

pub(in crate::chunk::evidence::rust) fn strip_raw(text: &str) -> Option<&str> {
    let stripped = text.strip_prefix("r#").unwrap_or(text);
    (!stripped.is_empty()).then_some(stripped)
}
