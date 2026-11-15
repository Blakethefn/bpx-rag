use tree_sitter::Node;

pub(in crate::chunk) fn is_module_level(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matches!(
            parent.kind(),
            "function_item"
                | "function_definition"
                | "function_declaration"
                | "impl_item"
                | "class_definition"
                | "class_body"
                | "method_declaration"
        ) {
            return false;
        }
        ancestor = parent.parent();
    }
    true
}

pub(super) fn is_declaration_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.child_by_field_name("name") != Some(node) {
        return false;
    }
    matches!(
        parent.kind(),
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "function_signature"
            | "struct_item"
            | "enum_item"
            | "enum_variant"
            | "class_definition"
            | "class_declaration"
            | "method_definition"
            | "method_declaration"
            | "impl_item"
            | "trait_item"
            | "mod_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "variable_declarator"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "class_specifier"
            | "struct_specifier"
    )
}

pub(super) fn is_binding_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind().contains("parameter") {
        return true;
    }
    matches!(
        parent.kind(),
        "let_declaration" | "let_condition" | "for_expression"
    ) && (parent.child_by_field_name("pattern") == Some(node)
        || parent.child_by_field_name("name") == Some(node))
}

pub(in crate::chunk) fn is_literal(kind: &str) -> bool {
    matches!(
        kind,
        "string"
            | "string_literal"
            | "string_content"
            | "interpreted_string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "line_comment"
            | "block_comment"
            | "comment"
    )
}

pub(super) fn is_noise(specifier: &str) -> bool {
    matches!(
        specifier,
        "self" | "Self" | "super" | "crate" | "cls" | "this" | "_" | "__name__"
    )
}
