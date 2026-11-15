use tree_sitter::Node;

use crate::relations::Grammar;

pub(super) fn is_boundary(node: Node<'_>, grammar: Grammar, kind: &str) -> bool {
    match grammar {
        Grammar::Rust => matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "impl_item"
                | "trait_item"
                | "mod_item"
                | "type_item"
                | "const_item"
                | "static_item"
                | "macro_definition"
        ),
        Grammar::C | Grammar::Cpp => {
            kind == "function_definition"
                || (matches!(
                    kind,
                    "struct_specifier" | "enum_specifier" | "class_specifier"
                ) && is_type_definition_context(node))
                || (grammar == Grammar::Cpp && kind == "namespace_definition")
        }
        Grammar::CSharp => matches!(
            kind,
            "namespace_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "constructor_declaration"
                | "property_declaration"
                | "delegate_declaration"
        ),
        Grammar::Go => matches!(
            kind,
            "function_declaration" | "method_declaration" | "type_declaration"
        ),
        _ => false,
    }
}

fn is_type_definition_context(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matches!(
            parent.kind(),
            "function_definition"
                | "compound_statement"
                | "parameter_declaration"
                | "parameter_list"
        ) {
            return false;
        }
        if matches!(
            parent.kind(),
            "type_definition"
                | "declaration"
                | "field_declaration"
                | "translation_unit"
                | "source_file"
        ) {
            return node.child_by_field_name("body").is_some()
                || (0..node.named_child_count() as u32).any(|index| {
                    node.named_child(index)
                        .is_some_and(|child| child.kind().ends_with("declaration_list"))
                });
        }
        ancestor = parent.parent();
    }
    false
}
