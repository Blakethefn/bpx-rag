mod c_family;
mod modules;
mod python;
mod scripting;
mod web;

use std::path::Path;

use tree_sitter::Node;

use super::super::super::languages::Grammar;
use super::super::super::metadata::ProjectMetadata;
use super::super::super::types::SourceFile;

pub(super) fn extract_specifiers(
    grammar: Grammar,
    node: Node<'_>,
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
) -> Vec<(&'static str, String)> {
    match grammar {
        Grammar::C | Grammar::Cpp => c_family::extract(grammar, node, &source.content),
        Grammar::Python => python::extract(node, source, root, files, metadata),
        Grammar::Go
        | Grammar::Java
        | Grammar::CSharp
        | Grammar::Kotlin
        | Grammar::Scala
        | Grammar::Haskell
        | Grammar::Erlang
        | Grammar::Solidity => modules::extract(grammar, node, &source.content),
        Grammar::Php
        | Grammar::Ruby
        | Grammar::Bash
        | Grammar::Lua
        | Grammar::Dart
        | Grammar::Swift
        | Grammar::Elixir
        | Grammar::R
        | Grammar::Julia
        | Grammar::Perl
        | Grammar::PowerShell => scripting::extract(grammar, node, &source.content),
        Grammar::Html | Grammar::Css => web::extract(grammar, node, &source.content),
        Grammar::Markdown
        | Grammar::Rust
        | Grammar::JavaScript
        | Grammar::TypeScript
        | Grammar::Tsx
        | Grammar::Sql => Vec::new(),
    }
}

pub(super) fn quoted_literal(value: &str) -> Option<String> {
    let value = value.trim();
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') {
        return None;
    }
    let end = value[quote.len_utf8()..].find(quote)? + quote.len_utf8();
    Some(value[quote.len_utf8()..end].to_string())
}

pub(super) fn quoted_prefix(value: &str) -> Option<String> {
    let start = value.find(['\'', '"', '`'])?;
    quoted_literal(&value[start..])
}
