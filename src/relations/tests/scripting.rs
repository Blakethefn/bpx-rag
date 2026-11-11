use super::*;

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
