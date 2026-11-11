use super::*;

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
