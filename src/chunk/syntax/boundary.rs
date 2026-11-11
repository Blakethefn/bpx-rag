use tree_sitter::Node;

pub(in crate::chunk) fn is_declaration_wrapper(kind: &str) -> bool {
    matches!(
        kind,
        "decorated_definition" | "export_statement" | "export_declaration" | "template_declaration"
    )
}

pub(super) fn is_declaration_boundary(
    node: Node<'_>,
    grammar: crate::relations::Grammar,
    content: &str,
) -> bool {
    let kind = node.kind();
    use crate::relations::Grammar;

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
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            is_declaration_wrapper(kind)
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
        Grammar::Java => matches!(
            kind,
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "method_declaration"
                | "constructor_declaration"
        ),
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
        Grammar::Dart => matches!(
            kind,
            "class_definition"
                | "enum_declaration"
                | "mixin_declaration"
                | "extension_declaration"
                | "function_declaration"
                | "method_signature"
        ),
        Grammar::Swift => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "enum_declaration"
                | "protocol_declaration"
                | "extension_declaration"
        ),
        Grammar::Kotlin => matches!(
            kind,
            "function_declaration" | "class_declaration" | "object_declaration"
        ),
        Grammar::Sql => kind == "statement",
        Grammar::Scala => matches!(
            kind,
            "class_definition"
                | "trait_definition"
                | "object_definition"
                | "enum_definition"
                | "function_definition"
                | "given_definition"
                | "extension_definition"
                | "package_object"
        ),
        Grammar::Haskell => matches!(
            kind,
            "declaration"
                | "function"
                | "data_type"
                | "newtype"
                | "type_synonym"
                | "class"
                | "instance"
                | "foreign_import"
        ),
        Grammar::Elixir => is_elixir_definition(node, content),
        Grammar::Erlang => matches!(kind, "function_clause" | "ssr_definition"),
        Grammar::R => kind == "function_definition",
        Grammar::Julia => matches!(
            kind,
            "function_definition"
                | "macro_definition"
                | "module_definition"
                | "struct_definition"
                | "abstract_definition"
                | "primitive_definition"
        ),
        Grammar::Perl => matches!(
            kind,
            "package_statement" | "function_definition" | "function_definition_without_sub"
        ),
        Grammar::PowerShell => {
            matches!(
                kind,
                "function_statement" | "class_statement" | "enum_statement"
            )
        }
        Grammar::Solidity => matches!(
            kind,
            "contract_declaration"
                | "interface_declaration"
                | "library_declaration"
                | "function_definition"
                | "constructor_definition"
                | "modifier_definition"
                | "struct_declaration"
                | "enum_declaration"
                | "event_definition"
                | "error_declaration"
                | "user_defined_type_definition"
        ),
        Grammar::Html => {
            matches!(kind, "element" | "script_element" | "style_element")
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "document")
        }
        Grammar::Css => {
            matches!(
                kind,
                "rule_set"
                    | "at_rule"
                    | "charset_statement"
                    | "import_statement"
                    | "keyframes_statement"
                    | "media_statement"
                    | "namespace_statement"
                    | "scope_statement"
                    | "supports_statement"
                    | "declaration"
            ) && node
                .parent()
                .is_some_and(|parent| parent.kind() == "stylesheet")
        }
        Grammar::Markdown => false,
    }
}

pub(super) fn is_elixir_definition(node: Node<'_>, content: &str) -> bool {
    if node.kind() != "call" {
        return false;
    }
    node.child_by_field_name("target")
        .and_then(|target| target.utf8_text(content.as_bytes()).ok())
        .is_some_and(|target| {
            matches!(
                target.trim(),
                "defmodule"
                    | "defprotocol"
                    | "defimpl"
                    | "def"
                    | "defp"
                    | "defmacro"
                    | "defmacrop"
                    | "defguard"
                    | "defguardp"
            )
        })
}

pub(super) fn is_type_definition_context(node: Node<'_>) -> bool {
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

pub(super) fn contains_function_value(node: Node<'_>) -> bool {
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

