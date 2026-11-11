use super::*;

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
