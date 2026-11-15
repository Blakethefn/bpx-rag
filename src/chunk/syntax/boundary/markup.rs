use tree_sitter::Node;

use crate::relations::Grammar;

pub(super) fn is_boundary(node: Node<'_>, grammar: Grammar, kind: &str, content: &str) -> bool {
    match grammar {
        Grammar::Sql => kind == "statement",
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
        _ => false,
    }
}

fn is_elixir_definition(node: Node<'_>, content: &str) -> bool {
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
