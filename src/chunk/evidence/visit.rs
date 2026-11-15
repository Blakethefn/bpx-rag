use std::path::Path;

use tree_sitter::Node;

use super::{is_literal, names, push_reference, EntryObservation, Reference, Walk};
use crate::relations::Grammar;

pub(super) fn visit(
    grammar: Grammar,
    path: &Path,
    node: Node<'_>,
    content: &str,
    references: &mut Vec<Reference>,
    entries: &mut Vec<EntryObservation>,
) -> bool {
    if is_literal(node.kind()) {
        return false;
    }
    let claimed = match grammar {
        Grammar::Rust => super::rust::visit(node, content, references, entries),
        Grammar::Python => super::python::visit(node, content, references, entries),
        Grammar::Go => super::go::visit(path, node, content, references, entries),
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            super::javascript::visit(node, content, references)
        }
        _ => Walk::Skip,
    };
    if claimed == Walk::Stop {
        return false;
    }

    if let names::Qualified::Stop(reference) = names::qualified_reference(node, content) {
        if let Some(reference) =
            reference.and_then(|reference| names::normalize_rust(grammar, reference))
        {
            push_reference(references, reference);
        }
        return false;
    }

    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier" | "property_identifier"
    ) {
        if let Some(reference) = names::plain_reference(node, content)
            .and_then(|reference| names::normalize_rust(grammar, reference))
        {
            push_reference(references, reference);
        }
        return false;
    }
    true
}
