use crate::relations::languages::Grammar;

use super::{listed, references};

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
