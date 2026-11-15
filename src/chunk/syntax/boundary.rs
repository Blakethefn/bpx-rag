use tree_sitter::Node;

mod compiled;
mod jvm;
mod markup;
mod script;

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
        Grammar::Rust | Grammar::C | Grammar::Cpp | Grammar::CSharp | Grammar::Go => {
            compiled::is_boundary(node, grammar, kind)
        }
        Grammar::JavaScript
        | Grammar::TypeScript
        | Grammar::Tsx
        | Grammar::Python
        | Grammar::Php
        | Grammar::Ruby
        | Grammar::Bash
        | Grammar::Lua => script::is_boundary(node, grammar, kind),
        Grammar::Java | Grammar::Kotlin | Grammar::Scala | Grammar::Dart | Grammar::Swift => {
            jvm::is_boundary(grammar, kind)
        }
        Grammar::Sql
        | Grammar::Haskell
        | Grammar::Elixir
        | Grammar::Erlang
        | Grammar::R
        | Grammar::Julia
        | Grammar::Perl
        | Grammar::PowerShell
        | Grammar::Solidity
        | Grammar::Html
        | Grammar::Css
        | Grammar::Markdown => markup::is_boundary(node, grammar, kind, content),
    }
}
