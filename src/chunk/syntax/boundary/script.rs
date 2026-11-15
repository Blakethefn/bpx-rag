use tree_sitter::Node;

use crate::relations::Grammar;

pub(super) fn is_boundary(node: Node<'_>, grammar: Grammar, kind: &str) -> bool {
    match grammar {
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            super::is_declaration_wrapper(kind)
                || matches!(
                    kind,
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                )
                || (kind == "method_definition"
                    && node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "class_body"))
                || (matches!(kind, "lexical_declaration" | "variable_declaration")
                    && contains_function_value(node))
        }
        Grammar::Python => matches!(
            kind,
            "decorated_definition" | "function_definition" | "class_definition"
        ),
        Grammar::Php => matches!(
            kind,
            "function_definition"
                | "class_declaration"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "property_declaration"
        ),
        Grammar::Ruby => matches!(kind, "class" | "module" | "method" | "singleton_method"),
        Grammar::Bash => kind == "function_definition",
        Grammar::Lua => kind == "function_declaration",
        _ => false,
    }
}

fn contains_function_value(node: Node<'_>) -> bool {
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if matches!(current.kind(), "arrow_function" | "function_expression") {
            return true;
        }
        for index in (0..current.named_child_count() as u32).rev() {
            if let Some(child) = current.named_child(index) {
                pending.push(child);
            }
        }
    }
    false
}
