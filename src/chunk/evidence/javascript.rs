//! JavaScript and TypeScript import paths and named imports.

use tree_sitter::Node;

use super::{is_literal, push_reference, reference, Walk, KIND_IMPORT, KIND_PATH};

pub(super) fn visit(node: Node<'_>, content: &str, references: &mut Vec<super::Reference>) -> Walk {
    if node.kind() != "import_statement" {
        return Walk::Skip;
    }
    import_statement(node, content, references);
    Walk::Stop
}

fn import_statement(node: Node<'_>, content: &str, references: &mut Vec<super::Reference>) {
    let Some(text) = node.utf8_text(content.as_bytes()).ok() else {
        return;
    };
    let Some(specifier) = quoted_specifier(text) else {
        return;
    };
    push_reference(references, reference(KIND_PATH, &specifier, None, node));
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if current.kind() == "import_specifier" {
            if let Some(name) = super::field_text(current, "name", content) {
                push_reference(
                    references,
                    reference(KIND_IMPORT, name, Some(specifier.clone()), node),
                );
            }
            continue;
        }
        if current.id() != node.id() && is_literal(current.kind()) {
            continue;
        }
        for index in 0..current.named_child_count() {
            if let Some(child) = current.named_child(index as u32) {
                pending.push(child);
            }
        }
    }
}

fn quoted_specifier(text: &str) -> Option<String> {
    let start = text.find(['\'', '"', '`'])?;
    let quote = text[start..].chars().next()?;
    let rest = &text[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    let specifier = &rest[..end];
    (!specifier.is_empty()).then(|| specifier.to_string())
}
