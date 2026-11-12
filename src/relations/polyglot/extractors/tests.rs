use std::path::{Path, PathBuf};

use super::super::super::super::languages::Grammar;
use super::super::super::super::metadata::ProjectMetadata;
use super::super::super::super::parsers::ParserCache;
use super::super::super::super::types::SourceFile;

fn references(grammar: Grammar, content: &str) -> Vec<(String, String)> {
    let mut parsers = ParserCache::default();
    let tree = parsers
        .parse(grammar, content)
        .unwrap_or_else(|| panic!("parse {grammar:?}"));
    let source = SourceFile {
        path: PathBuf::from("sample"),
        content: content.to_string(),
    };
    let metadata = ProjectMetadata::default();
    let mut pending = vec![tree.root_node()];
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        for (kind, specifier) in
            super::extract_specifiers(grammar, node, &source, Path::new("."), &[], &metadata)
        {
            found.push((kind.to_string(), specifier));
        }
        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    found.sort();
    found
}

fn listed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut found = pairs
        .iter()
        .map(|(kind, specifier)| (kind.to_string(), specifier.to_string()))
        .collect::<Vec<_>>();
    found.sort();
    found
}

#[test]
fn module_grammars_keep_their_import_shapes() {
    assert_eq!(
        references(
            Grammar::Go,
            "package main\nimport \"fmt\"\nimport (\n\t\"example.com/app/model\"\n)\n",
        ),
        listed(&[("import", "example.com/app/model"), ("import", "fmt"),]),
    );
    assert_eq!(
        references(
            Grammar::Java,
            "import com.acme.model.Quote;\nimport static java.util.Collections.emptyList;\nimport java.util.*;\n",
        ),
        listed(&[
            ("import", "com.acme.model.Quote"),
            ("import", "java.util.*"),
            ("import", "java.util.Collections"),
        ]),
    );
    assert_eq!(
        references(
            Grammar::CSharp,
            "using Acme.Models;\nglobal using System.IO;\nusing static System.Math;\nusing Alias = System.Console;\n",
        ),
        listed(&[
            ("import", "Acme.Models"),
            ("import", "System.IO"),
            ("import", "System.Math"),
        ]),
    );
    assert_eq!(
        references(
            Grammar::Kotlin,
            "import com.acme.model.Quote\nimport com.acme.model.Quote as Q\nimport com.acme.model.*\n",
        ),
        listed(&[
            ("import", "com.acme.model"),
            ("import", "com.acme.model.Quote"),
            ("import", "com.acme.model.Quote"),
        ]),
    );
    assert_eq!(
        references(
            Grammar::Scala,
            "import foo.Bar\nimport foo.{A, B}\nimport foo._\nimport Bar as Baz\n//>using dep org.example\n",
        ),
        listed(&[
            ("import", "Bar"),
            ("import", "foo.A"),
            ("import", "foo.B"),
            ("import", "foo.Bar"),
            ("import", "foo/*"),
        ]),
    );
    assert_eq!(
        references(Grammar::Haskell, "import Data.List\n"),
        listed(&[("import", "Data.List")]),
    );
    assert_eq!(
        references(
            Grammar::Erlang,
            "-module(sample).\n-import(lists, [map/2]).\n-include(\"header.hrl\").\n-include_lib(\"kernel/include/file.hrl\").\n",
        ),
        listed(&[
            ("import", "lists"),
            ("include", "header.hrl"),
            ("include", "kernel/include/file.hrl"),
        ]),
    );
    assert_eq!(
        references(
            Grammar::Solidity,
            "import \"./Token.sol\";\nusing Lib for uint256;\n",
        ),
        listed(&[("import", "./Token.sol")]),
    );
}

#[test]
fn scripting_grammars_keep_their_reference_shapes() {
    assert_eq!(
        references(
            Grammar::Dart,
            "import 'package:sample_app/model.dart';\nexport 'package:sample_app/model.dart';\ninclude('not_an_import.dart');\n",
        ),
        listed(&[
            ("import", "package:sample_app/model.dart"),
            ("re-export", "package:sample_app/model.dart"),
        ]),
    );
    assert_eq!(
        references(
            Grammar::Swift,
            "import Acme\ninclude(\"not_an_import.swift\")\n",
        ),
        listed(&[("import", "Acme")]),
    );
    assert_eq!(
        references(
            Grammar::Php,
            "<?php\nrequire_once './shared.php';\ninclude 'file.php';\n",
        ),
        listed(&[("include", "./shared.php"), ("include", "file.php")]),
    );
    assert_eq!(
        references(Grammar::Ruby, "require 'json'\nrequire_relative 'shared'\n",),
        listed(&[("import", "json"), ("import", "./shared")]),
    );
    assert_eq!(
        references(Grammar::Bash, "source ./lib.sh\n. ./other.sh\n"),
        listed(&[("include", "./lib.sh"), ("include", "./other.sh")]),
    );
    assert_eq!(
        references(Grammar::Lua, "require('pkg.module')\nrequire \"plain\"\n"),
        listed(&[("import", "pkg.module"), ("import", "plain")]),
    );
    assert_eq!(
        references(
            Grammar::Elixir,
            "alias Foo.Bar\nimport Foo.Baz\nrequire Logger\nuse Phoenix.Router\n",
        ),
        listed(&[
            ("import", "Foo.Bar"),
            ("import", "Foo.Baz"),
            ("import", "Logger"),
            ("import", "Phoenix.Router"),
        ]),
    );
    assert_eq!(
        references(Grammar::R, "source(\"plot.R\")\nsys.source(\"util.R\")\n"),
        listed(&[("include", "plot.R"), ("include", "util.R")]),
    );
    assert_eq!(
        references(
            Grammar::Julia,
            "import Baz\nusing Foo.Bar\ninclude(\"file.jl\")\n",
        ),
        listed(&[
            ("import", "Baz"),
            ("import", "Foo.Bar"),
            ("include", "file.jl"),
        ]),
    );
    assert_eq!(
        references(Grammar::Perl, "use strict;\nrequire File::Path;\n"),
        listed(&[("import", "strict"), ("include", "File::Path")]),
    );
    assert_eq!(
        references(
            Grammar::PowerShell,
            "Import-Module ./Module.psm1\n. ./sourced.ps1\n",
        ),
        listed(&[("import", "./Module.psm1"), ("include", "./sourced.ps1")]),
    );
}

#[test]
fn web_grammars_keep_local_include_shapes() {
    assert_eq!(
        references(
            Grammar::Html,
            "<script src=\"app.js\"></script>\n<link href=\"app.css\">\n<a href=\"https://example.com\">x</a>\n<a href=\"#top\">x</a>\n",
        ),
        listed(&[("include", "app.css"), ("include", "app.js")]),
    );
    assert_eq!(
        references(
            Grammar::Css,
            "@import \"theme.css\";\n@import url(\"extra.css\");\n@import url(https://cdn.example/a.css);\n",
        ),
        listed(&[("include", "extra.css"), ("include", "theme.css")]),
    );
}
