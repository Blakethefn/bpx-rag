use std::collections::HashMap;

use tree_sitter::{Language, Parser, Tree};

use super::languages::Grammar;

#[derive(Default)]
pub(super) struct ParserCache {
    parsers: HashMap<Grammar, Parser>,
}

impl ParserCache {
    pub(super) fn parse(&mut self, grammar: Grammar, content: &str) -> Option<Tree> {
        if !self.parsers.contains_key(&grammar) {
            let language = language_for(grammar)?;
            let mut parser = Parser::new();
            parser.set_language(&language).ok()?;
            self.parsers.insert(grammar, parser);
        }
        self.parsers.get_mut(&grammar)?.parse(content, None)
    }
}

fn language_for(grammar: Grammar) -> Option<Language> {
    let language = match grammar {
        Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
        Grammar::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Grammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Grammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Grammar::C => tree_sitter_c::LANGUAGE.into(),
        Grammar::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Grammar::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Grammar::Go => tree_sitter_go::LANGUAGE.into(),
        Grammar::Java => tree_sitter_java::LANGUAGE.into(),
        Grammar::Python => tree_sitter_python::LANGUAGE.into(),
        Grammar::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Grammar::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Grammar::Bash => tree_sitter_bash::LANGUAGE.into(),
        Grammar::Lua => tree_sitter_lua::LANGUAGE.into(),
        Grammar::Dart => tree_sitter_dart::LANGUAGE.into(),
        Grammar::Swift => tree_sitter_swift::LANGUAGE.into(),
        Grammar::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        Grammar::Sql => tree_sitter_sequel::LANGUAGE.into(),
        Grammar::Scala => tree_sitter_scala::LANGUAGE.into(),
        Grammar::Haskell => tree_sitter_haskell::LANGUAGE.into(),
        Grammar::Elixir => tree_sitter_elixir::LANGUAGE.into(),
        Grammar::Erlang => tree_sitter_erlang::LANGUAGE.into(),
        Grammar::R => tree_sitter_r::LANGUAGE.into(),
        Grammar::Julia => tree_sitter_julia::LANGUAGE.into(),
        Grammar::Perl => tree_sitter_perl::LANGUAGE.into(),
        Grammar::PowerShell => tree_sitter_powershell::LANGUAGE.into(),
        Grammar::Solidity => tree_sitter_solidity::LANGUAGE.into(),
        Grammar::Html => tree_sitter_html::LANGUAGE.into(),
        Grammar::Css => tree_sitter_css::LANGUAGE.into(),
        Grammar::Markdown => return None,
    };
    Some(language)
}
