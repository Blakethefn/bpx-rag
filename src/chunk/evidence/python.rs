//! Python imports, the `__main__` guard, and `test_` functions.

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
    if node.kind() == "import_statement" {
        import_statement(node, content, references);
        return Walk::Stop;
    }
    if node.kind() == "import_from_statement" {
        import_from(node, content, references);
        return Walk::Stop;
    }
    if node.kind() == "if_statement" && main_guard(node, content) {
        push_entry(
            entries,
            EntryObservation {
                target_rel_path: None,
                kind: ENTRY_PROGRAM,
                reason: "if __name__ == \"__main__\"",
            },
        );
    }
    if node.kind() == "function_definition"
        && is_module_level(node)
        && field_text(node, "name", content).is_some_and(|name| name.starts_with("test_"))
    {
        push_entry(
            entries,
            EntryObservation {
                target_rel_path: None,
                kind: ENTRY_TEST,
                reason: "test function",
            },
        );
    }
    Walk::Skip
}

fn import_statement(node: Node<'_>, content: &str, references: &mut Vec<super::Reference>) {
    let Some(text) = node.utf8_text(content.as_bytes()).ok() else {
        return;
    };
    let Some(rest) = text.trim().strip_prefix("import") else {
        return;
    };
    for part in rest.split(',') {
        let Some(name) = part.trim().split_whitespace().next() else {
            continue;
        };
        if name.is_empty() || name == "as" {
            continue;
        }
        push_reference(references, reference(KIND_MODULE, name, None, node));
    }
}

fn import_from(node: Node<'_>, content: &str, references: &mut Vec<super::Reference>) {
    let module = node
        .child_by_field_name("module_name")
        .and_then(|module| node_text(module, content))
        .map(str::to_string);
    if let Some(module) = module.as_deref() {
        if !module.is_empty() {
            push_reference(references, reference(KIND_MODULE, module, None, node));
        }
    }
    let Ok(child_count) = u32::try_from(node.child_count()) else {
        return;
    };
    for index in 0..child_count {
        if node.field_name_for_child(index) != Some("name") {
            continue;
        }
        let Some(name_node) = node.child(index) else {
            continue;
        };
        let Some(name) =
            node_text(name_node, content).and_then(|text| text.split_whitespace().next())
        else {
            continue;
        };
        if name.is_empty() || name == "*" {
            continue;
        }
        push_reference(
            references,
            reference(KIND_IMPORT, name, module.clone(), node),
        );
    }
}

fn main_guard(node: Node<'_>, content: &str) -> bool {
    node.child_by_field_name("condition")
        .and_then(|condition| condition.utf8_text(content.as_bytes()).ok())
        .is_some_and(|text| text.contains("__name__") && text.contains("__main__"))
}
