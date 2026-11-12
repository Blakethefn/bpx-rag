use std::fs;

use tempfile::tempdir;

use super::resolve_script_import;

fn root_with(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for (relative, contents) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }
    (dir, root)
}

#[test]
fn extensionless_import_prefers_typescript_over_javascript() {
    let (_dir, root) = root_with(&[
        ("src/app.ts", "export {}\n"),
        ("src/app.js", "export {}\n"),
        ("src/main.ts", ""),
    ]);

    let found = resolve_script_import(&root.join("src/main.ts"), &root, "./app").unwrap();

    assert_eq!(found, root.join("src/app.ts"));
}

#[test]
fn javascript_specifier_prefers_the_javascript_file() {
    let (_dir, root) = root_with(&[
        ("src/model.ts", ""),
        ("src/model.js", ""),
        ("src/consumer.js", ""),
    ]);

    let found = resolve_script_import(&root.join("src/consumer.js"), &root, "./model.js").unwrap();

    assert_eq!(found, root.join("src/model.js"));
}

#[test]
fn javascript_specifier_resolves_to_typescript_when_javascript_is_absent() {
    let (_dir, root) = root_with(&[("src/model.ts", ""), ("src/consumer.js", "")]);

    let found = resolve_script_import(&root.join("src/consumer.js"), &root, "./model.js").unwrap();

    assert_eq!(found, root.join("src/model.ts"));
}

#[test]
fn javascript_specifier_does_not_resolve_to_json() {
    let (_dir, root) = root_with(&[("src/model.json", "{}"), ("src/consumer.js", "")]);

    let found = resolve_script_import(&root.join("src/consumer.js"), &root, "./model.js");

    assert_eq!(found, None);
}

#[test]
fn extensionless_import_resolves_json_after_source_extensions() {
    let (_dir, root) = root_with(&[("src/config.json", "{}"), ("src/main.ts", "")]);

    let found = resolve_script_import(&root.join("src/main.ts"), &root, "./config").unwrap();

    assert_eq!(found, root.join("src/config.json"));
}

#[test]
fn extensionless_import_finds_index_typescript_before_index_javascript() {
    let (_dir, root) = root_with(&[
        ("src/lib/index.ts", ""),
        ("src/lib/index.js", ""),
        ("src/main.ts", ""),
    ]);

    let found = resolve_script_import(&root.join("src/main.ts"), &root, "./lib").unwrap();

    assert_eq!(found, root.join("src/lib/index.ts"));
}
