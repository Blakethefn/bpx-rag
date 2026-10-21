use super::types::{FileAnalysis, FileRelationship};
use super::*;
use std::fs;
use tempfile::tempdir;

fn write_file(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn incoming_from<'a>(result: &'a FileAnalysis, from: &str, specifier: &str, kind: &str) -> bool {
    result.incoming.iter().any(|relationship| {
        relationship.from == from
            && relationship.specifier == specifier
            && relationship.kind == kind
    })
}

#[test]
fn markdown_wikilinks_return_outgoing_and_incoming_line_evidence() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.md"), "# Alpha\nSee [[beta|Beta]].\n").unwrap();
    fs::write(dir.path().join("beta.md"), "Back to [[alpha]].\n").unwrap();

    let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert_eq!(result.outgoing.len(), 1);
    assert_eq!(result.outgoing[0].to.as_deref(), Some("beta.md"));
    assert_eq!(result.outgoing[0].line, 2);
    assert_eq!(result.incoming.len(), 1);
    assert_eq!(result.incoming[0].from, "beta.md");
    assert_eq!(result.incoming[0].line, 1);
}

#[test]
fn typescript_relative_imports_find_incoming_files_from_the_current_filesystem() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/lib/api")).unwrap();
    fs::create_dir_all(dir.path().join("src/hooks")).unwrap();
    fs::write(dir.path().join("package.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("src/lib/api/quotes.ts"),
        "export function getQuotes() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/hooks/useQuotes.ts"),
        "import { getQuotes } from '../lib/api/quotes';\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/lib/api/quotes.ts"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 1);
    assert_eq!(result.incoming[0].from, "src/hooks/useQuotes.ts");
    assert_eq!(result.incoming[0].kind, "import");
}

#[test]
fn typescript_import_meta_does_not_create_a_false_import_edge() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("src/quotes.ts"),
        "export const quote = 1;\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/useQuotes.ts"),
        "const url = import.meta.url\nimport { quote } from './quotes';\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/quotes.ts"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 1);
    assert_eq!(result.incoming[0].from, "src/useQuotes.ts");
    assert_eq!(result.incoming[0].line, 2);
}

#[test]
fn javascript_extension_imports_resolve_to_typescript_and_prefer_javascript_files() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "package.json", "{}\n");
    write_file(dir.path(), "src/model.ts", "export const model = 1;\n");
    write_file(
        dir.path(),
        "src/consumer-ts.ts",
        "import { model } from './model.js';\n",
    );

    let typescript = analyze_file(&dir.path().join("src/model.ts"), Some(dir.path())).unwrap();
    assert!(incoming_from(
        &typescript,
        "src/consumer-ts.ts",
        "./model.js",
        "import"
    ));

    write_file(dir.path(), "src/model.js", "export const model = 2;\n");
    write_file(
        dir.path(),
        "src/consumer-js.js",
        "import { model } from './model.js';\n",
    );
    let javascript = analyze_file(&dir.path().join("src/model.js"), Some(dir.path())).unwrap();
    let typescript_with_javascript =
        analyze_file(&dir.path().join("src/model.ts"), Some(dir.path())).unwrap();

    assert!(!incoming_from(
        &typescript_with_javascript,
        "src/consumer-js.js",
        "./model.js",
        "import"
    ));
    assert!(incoming_from(
        &javascript,
        "src/consumer-js.js",
        "./model.js",
        "import"
    ));
}

#[test]
fn python_imported_symbols_do_not_match_unrelated_same_named_files() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "def helper(): pass\n");
    write_file(dir.path(), "helper.py", "def helper(): pass\n");
    write_file(dir.path(), "consumer.py", "from pkg import helper\n");

    let result = analyze_file(&dir.path().join("helper.py"), Some(dir.path())).unwrap();

    assert!(!incoming_from(
        &result,
        "consumer.py",
        "pkg.helper",
        "import"
    ));
}

#[test]
fn rust_module_declarations_find_the_declared_file() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/api")).unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub mod api;\n").unwrap();
    fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
    fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();

    let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 1);
    assert_eq!(result.incoming[0].from, "src/api.rs");
    assert_eq!(result.incoming[0].kind, "module");
}

#[test]
fn next_analysis_reads_current_filesystem_state() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.md"), "No links yet.\n").unwrap();
    fs::write(dir.path().join("beta.md"), "\n").unwrap();

    let before = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();
    fs::write(dir.path().join("alpha.md"), "[[beta]]\n").unwrap();
    let after = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert!(before.outgoing.is_empty());
    assert_eq!(after.outgoing.len(), 1);
}

#[test]
fn file_outside_root_is_rejected() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("outside.md");
    fs::write(&target, "text\n").unwrap();

    assert!(analyze_file(&target, Some(dir.path())).is_err());
}

#[test]
fn formatted_output_groups_repeated_references_to_the_same_file() {
    let analysis = FileAnalysis {
        root: "/tmp/project".to_string(),
        file: "src/quotes.ts".to_string(),
        scanned_files: 2,
        outgoing: vec![
            FileRelationship {
                from: "src/quotes.ts".to_string(),
                to: Some("src/types.ts".to_string()),
                kind: "import".to_string(),
                specifier: "./types".to_string(),
                line: 1,
                direction: "outgoing".to_string(),
            },
            FileRelationship {
                from: "src/quotes.ts".to_string(),
                to: Some("src/types.ts".to_string()),
                kind: "import".to_string(),
                specifier: "./types".to_string(),
                line: 2,
                direction: "outgoing".to_string(),
            },
        ],
        incoming: Vec::new(),
        warnings: Vec::new(),
    };

    let rendered = format_analysis(&analysis);

    assert!(rendered.contains("Outgoing (1)"));
    assert!(rendered.contains("lines 1, 2"));
    assert!(rendered.contains("Incoming (0)"));
}

#[test]
fn rust_use_paths_resolve_to_module_files() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/api")).unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
    fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();
    fs::write(
        dir.path().join("src/service.rs"),
        "use crate::api::quotes::Quote;\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 2);
    assert!(result.incoming.iter().any(|relationship| {
        relationship.from == "src/service.rs"
            && relationship.line == 1
            && relationship.kind == "import"
    }));
    assert!(result.incoming.iter().any(|relationship| {
        relationship.from == "src/api.rs" && relationship.kind == "module"
    }));
}

#[test]
fn rust_grouped_use_paths_resolve_nested_modules() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/api")).unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(dir.path().join("src/api.rs"), "pub mod quotes;\n").unwrap();
    fs::write(dir.path().join("src/api/quotes.rs"), "pub struct Quote;\n").unwrap();
    fs::write(
        dir.path().join("src/service.rs"),
        "use crate::{api::quotes::Quote};\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/api/quotes.rs"), Some(dir.path())).unwrap();

    assert!(result.incoming.iter().any(|relationship| {
        relationship.from == "src/service.rs" && relationship.kind == "import"
    }));
}

#[test]
fn ignored_directories_are_not_scanned_for_incoming_references() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
    fs::write(dir.path().join("src/alpha.md"), "# Alpha\n").unwrap();
    fs::write(
        dir.path().join("node_modules/pkg/ref.md"),
        "[[../../src/alpha]]\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/alpha.md"), Some(dir.path())).unwrap();

    assert!(result.incoming.is_empty());
}

#[test]
fn selected_ignored_file_still_returns_its_own_outgoing_links() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "alpha.md\n").unwrap();
    fs::write(dir.path().join("alpha.md"), "See [[beta]].\n").unwrap();
    fs::write(dir.path().join("beta.md"), "# Beta\n").unwrap();

    let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert_eq!(result.outgoing.len(), 1);
    assert_eq!(result.outgoing[0].to.as_deref(), Some("beta.md"));
}

#[test]
fn unresolved_markdown_links_are_reported_without_guessing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.md"), "See [[missing-note]].\n").unwrap();

    let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert_eq!(result.outgoing.len(), 1);
    assert_eq!(result.outgoing[0].to, None);
    assert_eq!(result.outgoing[0].specifier, "missing-note");
}

#[test]
fn relative_parent_wikilinks_resolve_when_they_stay_inside_the_root() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("alpha.md"), "# Alpha\n").unwrap();
    fs::write(dir.path().join("docs/guide.md"), "See [[../alpha]].\n").unwrap();

    let result = analyze_file(&dir.path().join("docs/guide.md"), Some(dir.path())).unwrap();

    assert_eq!(result.outgoing[0].to.as_deref(), Some("alpha.md"));
}

#[test]
fn c_and_cpp_local_includes_resolve_and_ignore_comments_and_strings() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "include/header.h", "#pragma once\n");
    write_file(
        dir.path(),
        "src/main.c",
        "#include \"../include/header.h\"\n// #include \"missing.h\"\nconst char *s = \"#include \\\"also_missing.h\\\"\";\n",
    );
    write_file(
        dir.path(),
        "src/main.cpp",
        "#include \"../include/header.h\"\n/* #include \"comment_only.hpp\" */\n",
    );

    let result = analyze_file(&dir.path().join("include/header.h"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 2);
    assert!(incoming_from(
        &result,
        "src/main.c",
        "../include/header.h",
        "include"
    ));
    assert!(incoming_from(
        &result,
        "src/main.cpp",
        "../include/header.h",
        "include"
    ));
}

#[test]
fn python_import_lists_and_from_imports_resolve_each_module() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "");
    write_file(dir.path(), "pkg/alpha.py", "");
    write_file(dir.path(), "pkg/beta.py", "");
    write_file(dir.path(), "pkg/sub/__init__.py", "");
    write_file(dir.path(), "pkg/sub/gamma.py", "");
    write_file(
        dir.path(),
        "consumer.py",
        "import pkg.alpha, pkg.beta\nfrom pkg.sub.gamma import Thing\n",
    );

    for (target, specifier) in [
        ("pkg/alpha.py", "pkg.alpha"),
        ("pkg/beta.py", "pkg.beta"),
        ("pkg/sub/gamma.py", "pkg.sub.gamma"),
    ] {
        let result = analyze_file(&dir.path().join(target), Some(dir.path())).unwrap();
        assert!(
            incoming_from(&result, "consumer.py", specifier, "import"),
            "{target}"
        );
    }
}

#[test]
fn go_imports_stay_within_the_declared_module_boundary() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "go.mod", "module example.com/app\n\ngo 1.23\n");
    write_file(dir.path(), "internal/model/model.go", "package model\n");
    write_file(
        dir.path(),
        "consumer/consumer.go",
        "package consumer\nimport (\n \"example.com/app/internal/model\"\n \"example.com/application/internal/model\"\n)\n",
    );

    let result = analyze_file(
        &dir.path().join("internal/model/model.go"),
        Some(dir.path()),
    )
    .unwrap();

    assert_eq!(result.incoming.len(), 1);
    assert!(incoming_from(
        &result,
        "consumer/consumer.go",
        "example.com/app/internal/model",
        "import"
    ));
}

#[test]
fn java_and_kotlin_package_imports_match_package_paths() {
    let dir = tempdir().unwrap();
    write_file(
        dir.path(),
        "java/com/acme/model/Quote.java",
        "package com.acme.model;\n",
    );
    write_file(
        dir.path(),
        "java/com/acme/service/Consumer.java",
        "package com.acme.service;\nimport com.acme.model.Quote;\n",
    );
    write_file(
        dir.path(),
        "kotlin/com/acme/model/Quote.kt",
        "package com.acme.model\n",
    );
    write_file(
        dir.path(),
        "kotlin/com/acme/service/Consumer.kt",
        "package com.acme.service\nimport com.acme.model.Quote\n",
    );

    let java = analyze_file(
        &dir.path().join("java/com/acme/model/Quote.java"),
        Some(dir.path()),
    )
    .unwrap();
    let kotlin = analyze_file(
        &dir.path().join("kotlin/com/acme/model/Quote.kt"),
        Some(dir.path()),
    )
    .unwrap();

    assert!(incoming_from(
        &java,
        "java/com/acme/service/Consumer.java",
        "com.acme.model.Quote",
        "import"
    ));
    assert!(incoming_from(
        &kotlin,
        "kotlin/com/acme/service/Consumer.kt",
        "com.acme.model.Quote",
        "import"
    ));
}

#[test]
fn dart_package_imports_and_exports_resolve_only_the_own_package() {
    let dir = tempdir().unwrap();
    write_file(
        dir.path(),
        "pubspec.yaml",
        "name: sample_app\nenvironment:\n  sdk: '>=3.0.0'\n",
    );
    write_file(dir.path(), "lib/model.dart", "class Model {}\n");
    write_file(
        dir.path(),
        "lib/consumer.dart",
        "import 'package:sample_app/model.dart';\nexport 'package:sample_app/model.dart';\nimport 'package:third_party/model.dart';\n",
    );

    let result = analyze_file(&dir.path().join("lib/model.dart"), Some(dir.path())).unwrap();

    assert_eq!(result.incoming.len(), 2);
    assert!(incoming_from(
        &result,
        "lib/consumer.dart",
        "package:sample_app/model.dart",
        "import"
    ));
    assert!(incoming_from(
        &result,
        "lib/consumer.dart",
        "package:sample_app/model.dart",
        "re-export"
    ));
}

#[test]
fn php_relative_includes_and_ruby_require_relative_resolve_without_decoys() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "lib/shared.php", "<?php\n");
    write_file(
        dir.path(),
        "lib/consumer.php",
        "<?php\nrequire_once './shared.php';\n// include 'missing.php';\n$text = \"require 'also_missing.php'\";\n",
    );
    write_file(dir.path(), "ruby/lib/shared.rb", "VALUE = 1\n");
    write_file(
        dir.path(),
        "ruby/lib/consumer.rb",
        "require_relative 'shared'\n# require_relative 'missing'\ntext = \"require_relative 'also_missing'\"\n",
    );

    let php = analyze_file(&dir.path().join("lib/shared.php"), Some(dir.path())).unwrap();
    let ruby = analyze_file(&dir.path().join("ruby/lib/shared.rb"), Some(dir.path())).unwrap();

    assert_eq!(php.incoming.len(), 1);
    assert_eq!(php.incoming[0].from, "lib/consumer.php");
    assert!(php.incoming[0].kind == "include");
    assert_eq!(ruby.incoming.len(), 1);
    assert_eq!(ruby.incoming[0].from, "ruby/lib/consumer.rb");
    assert_eq!(ruby.incoming[0].specifier, "./shared");
}

#[test]
fn bash_source_and_lua_require_resolve_local_files_and_ignore_decoys() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "shell/lib.sh", "value=1\n");
    write_file(
        dir.path(),
        "shell/main.sh",
        "source ./lib.sh\n# source ./missing.sh\necho 'source ./also_missing.sh'\n",
    );
    write_file(dir.path(), "lua/pkg/module.lua", "return {}\n");
    write_file(
        dir.path(),
        "lua/main.lua",
        "local module = require('pkg.module')\n-- require('missing.module')\nlocal text = \"require('also.missing')\"\n",
    );

    let bash = analyze_file(&dir.path().join("shell/lib.sh"), Some(dir.path())).unwrap();
    let lua = analyze_file(&dir.path().join("lua/pkg/module.lua"), Some(dir.path())).unwrap();

    assert_eq!(bash.incoming.len(), 1);
    assert_eq!(bash.incoming[0].from, "shell/main.sh");
    assert_eq!(lua.incoming.len(), 1);
    assert_eq!(lua.incoming[0].from, "lua/main.lua");
    assert_eq!(lua.incoming[0].specifier, "pkg.module");
}

#[test]
fn swift_and_csharp_imports_map_namespaces_to_local_directories() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "swift/Acme/Quote.swift", "struct Quote {}\n");
    write_file(
        dir.path(),
        "swift/Consumer.swift",
        "import Acme\n// import Missing\nlet note = \"import AlsoMissing\"\n",
    );
    write_file(
        dir.path(),
        "csharp/Acme/Models/Quote.cs",
        "namespace Acme.Models;\n",
    );
    write_file(
        dir.path(),
        "csharp/Consumer.cs",
        "using Acme.Models;\n// using Missing.Namespace;\nvar note = \"using Also.Missing;\";\n",
    );

    let swift = analyze_file(&dir.path().join("swift/Acme/Quote.swift"), Some(dir.path())).unwrap();
    let csharp = analyze_file(
        &dir.path().join("csharp/Acme/Models/Quote.cs"),
        Some(dir.path()),
    )
    .unwrap();

    assert_eq!(swift.incoming.len(), 1, "{swift:?}");
    assert!(incoming_from(
        &swift,
        "swift/Consumer.swift",
        "Acme",
        "import"
    ));
    assert_eq!(csharp.incoming.len(), 1);
    assert!(incoming_from(
        &csharp,
        "csharp/Consumer.cs",
        "Acme.Models",
        "import"
    ));
}

#[test]
fn bare_quoted_c_includes_and_php_requires_resolve_from_the_source_directory() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "c/header.h", "#pragma once\n");
    write_file(dir.path(), "c/consumer.c", "#include \"header.h\"\n");
    write_file(dir.path(), "php/file.php", "<?php\n");
    write_file(
        dir.path(),
        "php/consumer.php",
        "<?php\nrequire 'file.php';\n",
    );

    let header = analyze_file(&dir.path().join("c/header.h"), Some(dir.path())).unwrap();
    let php = analyze_file(&dir.path().join("php/file.php"), Some(dir.path())).unwrap();

    assert_eq!(header.incoming.len(), 1);
    assert!(incoming_from(
        &header,
        "c/consumer.c",
        "header.h",
        "include"
    ));
    assert_eq!(php.incoming.len(), 1);
    assert!(incoming_from(
        &php,
        "php/consumer.php",
        "file.php",
        "include"
    ));
}

#[test]
fn unresolved_local_polyglot_imports_remain_outgoing_without_targets() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "src/main.c", "#include \"missing.h\"\n");
    write_file(
        dir.path(),
        "ruby/main.rb",
        "require_relative 'missing_file'\n",
    );

    let c = analyze_file(&dir.path().join("src/main.c"), Some(dir.path())).unwrap();
    let ruby = analyze_file(&dir.path().join("ruby/main.rb"), Some(dir.path())).unwrap();

    assert_eq!(c.outgoing.len(), 1);
    assert_eq!(c.outgoing[0].specifier, "missing.h");
    assert_eq!(c.outgoing[0].to, None);
    assert_eq!(ruby.outgoing.len(), 1);
    assert_eq!(ruby.outgoing[0].specifier, "./missing_file");
    assert_eq!(ruby.outgoing[0].to, None);
}

#[test]
fn python_from_package_import_resolves_module_without_inventing_symbol_edges() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "pkg/__init__.py", "");
    write_file(dir.path(), "pkg/quotes.py", "");
    write_file(
        dir.path(),
        "consumer.py",
        "from pkg import quotes, MissingSymbol\n",
    );

    let quotes = analyze_file(&dir.path().join("pkg/quotes.py"), Some(dir.path())).unwrap();
    let consumer = analyze_file(&dir.path().join("consumer.py"), Some(dir.path())).unwrap();

    assert!(incoming_from(
        &quotes,
        "consumer.py",
        "pkg.quotes",
        "import"
    ));
    assert!(consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg"));
    assert!(consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg.quotes"));
    assert!(!consumer
        .outgoing
        .iter()
        .any(|relationship| relationship.specifier == "pkg.MissingSymbol"));
}

#[test]
fn project_markers_infer_roots_for_go_java_dart_and_csharp() {
    let cases = [
        (
            "go",
            "go.mod",
            "module example.com/project\n",
            "src/main.go",
            "package main\n",
        ),
        (
            "java",
            "pom.xml",
            "<project></project>\n",
            "src/Main.java",
            "class Main {}\n",
        ),
        (
            "dart",
            "pubspec.yaml",
            "name: sample_app\n",
            "lib/main.dart",
            "void main() {}\n",
        ),
        (
            "csharp",
            "Sample.csproj",
            "<Project />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
        (
            "csharp directory props",
            "Directory.Build.props",
            "<Project />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
        (
            "csharp slnx",
            "Sample.slnx",
            "<Solution />\n",
            "src/Main.cs",
            "class Main {}\n",
        ),
    ];

    for (name, marker, marker_content, source, source_content) in cases {
        let dir = tempdir().unwrap();
        write_file(dir.path(), marker, marker_content);
        write_file(dir.path(), source, source_content);

        let result = analyze_file(&dir.path().join(source), None).unwrap();

        assert_eq!(
            result.root,
            dir.path().canonicalize().unwrap().display().to_string(),
            "{name}"
        );
    }
}
