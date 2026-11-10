//! Rust module declarations, `use` paths, `fn main`, and `#[test]`.

use tree_sitter::Node;

use super::{
    field_text, is_module_level, node_text, push_entry, push_reference, reference,
    EntryObservation, Walk, ENTRY_PROGRAM, ENTRY_TEST, KIND_IMPORT, KIND_MODULE,
};

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

fn collect_use(
    node: Node<'_>,
    prefix: &[String],
    content: &str,
    references: &mut Vec<super::Reference>,
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
    references: &mut Vec<super::Reference>,
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

fn path_segments(node: Node<'_>, content: &str) -> Option<Vec<String>> {
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

fn join_path(segments: &[String]) -> Option<String> {
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

fn strip_raw(text: &str) -> Option<&str> {
    let stripped = text.strip_prefix("r#").unwrap_or(text);
    (!stripped.is_empty()).then_some(stripped)
}

/// The nearest `#[path = "..."]` on this module. The stored value is the string contents.
/// An unreadable `path` attribute stores nothing rather than a guessed path.
fn mod_path(node: Node<'_>, content: &str) -> Option<String> {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if super::is_literal(current.kind()) {
            sibling = current.prev_named_sibling();
            continue;
        }
        if current.kind() != "attribute_item" {
            break;
        }
        match path_attribute(current, content) {
            PathAttribute::Value(path) => return Some(path),
            PathAttribute::Unreadable => return None,
            PathAttribute::Other => {}
        }
        sibling = current.prev_named_sibling();
    }
    None
}

enum PathAttribute {
    Value(String),
    Unreadable,
    Other,
}

fn path_attribute(item: Node<'_>, content: &str) -> PathAttribute {
    let Some(attribute) = item.named_child(0) else {
        return PathAttribute::Other;
    };
    if attribute.kind() != "attribute" {
        return PathAttribute::Other;
    }
    let Some(name) = attribute_name(attribute, content) else {
        return PathAttribute::Other;
    };
    if name != "path" {
        return PathAttribute::Other;
    }
    let Some(value) = attribute.child_by_field_name("value") else {
        return PathAttribute::Unreadable;
    };
    match literal_string(value, content) {
        Some(path) => PathAttribute::Value(path),
        None => PathAttribute::Unreadable,
    }
}

fn attribute_name<'a>(attribute: Node<'_>, content: &'a str) -> Option<&'a str> {
    let value = attribute.child_by_field_name("value");
    let arguments = attribute.child_by_field_name("arguments");
    for index in 0..attribute.named_child_count() {
        let child = attribute.named_child(index as u32)?;
        if Some(child) == value || Some(child) == arguments {
            continue;
        }
        return node_text(child, content);
    }
    None
}

fn literal_string(node: Node<'_>, content: &str) -> Option<String> {
    match node.kind() {
        "raw_string_literal" => {
            let inner = node.named_child(0)?;
            if inner.kind() != "string_content" {
                return None;
            }
            node_text(inner, content).map(str::to_string)
        }
        "string_literal" => {
            let mut value = String::new();
            if node.named_child_count() == 0 {
                return Some(value);
            }
            for index in 0..node.named_child_count() {
                let child = node.named_child(index as u32)?;
                match child.kind() {
                    "string_content" => value.push_str(node_text(child, content)?),
                    "escape_sequence" => {
                        value.push_str(&decoded_escape(node_text(child, content)?)?)
                    }
                    _ => return None,
                }
            }
            Some(value)
        }
        _ => None,
    }
}

fn decoded_escape(text: &str) -> Option<String> {
    match text {
        "\\n" => Some("\n".to_string()),
        "\\r" => Some("\r".to_string()),
        "\\t" => Some("\t".to_string()),
        "\\\\" => Some("\\".to_string()),
        "\\'" => Some("'".to_string()),
        "\\\"" => Some("\"".to_string()),
        "\\0" => Some("\0".to_string()),
        _ => decode_numeric_escape(text),
    }
}

fn decode_numeric_escape(text: &str) -> Option<String> {
    let code = if let Some(hex) = text.strip_prefix("\\x") {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        let body = text.strip_prefix("\\u{")?.strip_suffix('}')?;
        u32::from_str_radix(body, 16).ok()?
    };
    Some(char::from_u32(code)?.to_string())
}

fn attribute_is_test(item: Node<'_>, content: &str) -> bool {
    let Some(attribute) = item.named_child(0) else {
        return false;
    };
    if attribute.kind() != "attribute" {
        return false;
    }
    let Some(name) = attribute_name(attribute, content) else {
        return false;
    };
    name == "test" || name.ends_with("::test")
}

fn has_test_attribute(node: Node<'_>, content: &str) -> bool {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            break;
        }
        if attribute_is_test(current, content) {
            return true;
        }
        sibling = current.prev_named_sibling();
    }
    false
}
