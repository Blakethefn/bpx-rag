use std::path::Path;

use tree_sitter::Node;

use super::{
    field_text, is_module_level, push_entry, push_reference, reference, EntryObservation, Walk,
    ENTRY_PROGRAM, ENTRY_TEST, KIND_MODULE,
};

pub(super) fn visit(
    path: &Path,
    node: Node<'_>,
    content: &str,
    references: &mut Vec<super::Reference>,
    entries: &mut Vec<EntryObservation>,
) -> Walk {
    if node.kind() == "import_spec" {
        if let Some(specifier) = import_path(node, content) {
            push_reference(references, reference(KIND_MODULE, &specifier, None, node));
        }
        return Walk::Stop;
    }
    if node.kind() == "function_declaration" && is_module_level(node) {
        match field_text(node, "name", content) {
            Some("main") => push_entry(
                entries,
                EntryObservation {
                    target_rel_path: None,
                    kind: ENTRY_PROGRAM,
                    reason: "func main".to_string(),
                },
            ),
            Some(name)
                if name.starts_with("Test")
                    && path
                        .file_name()
                        .and_then(|file| file.to_str())
                        .is_some_and(|file| file.ends_with("_test.go")) =>
            {
                push_entry(
                    entries,
                    EntryObservation {
                        target_rel_path: None,
                        kind: ENTRY_TEST,
                        reason: "func Test".to_string(),
                    },
                );
            }
            _ => {}
        }
    }
    Walk::Skip
}

fn import_path(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?;
    let specifier = text.trim().trim_matches(['"', '`']).trim();
    (!specifier.is_empty()).then(|| specifier.to_string())
}
