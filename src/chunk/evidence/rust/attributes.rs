use tree_sitter::Node;

use super::super::{is_literal, node_text};

pub(super) fn mod_path(node: Node<'_>, content: &str) -> Option<String> {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if is_literal(current.kind()) {
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

pub(super) fn has_test_attribute(node: Node<'_>, content: &str) -> bool {
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
