use super::*;

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
