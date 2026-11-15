use crate::relations::languages::Grammar;

use super::{listed, references};

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
