use super::{analysis, file_reason, finding_rows, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn an_implicit_library_reaches_its_child_and_an_undeclared_file_is_unused() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "mod util;\npub fn exported() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn helper() {}\n").unwrap();
    fs::write(dir.path().join("src/legacy.rs"), "fn old() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("limitation:public_api"), "{gaps}");
    assert!(!gaps.contains("no_entry_points"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(file_reason(&findings, "src/lib.rs"), None, "{findings:?}");
    assert_eq!(file_reason(&findings, "src/util.rs"), None, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "src/legacy.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert!(!unused(&findings, "src/lib.rs", "exported"), "{findings:?}");
}

#[test]
fn modules_that_only_reach_each_other_are_detached() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/legacy")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    fs::write(dir.path().join("src/legacy/mod.rs"), "mod old;\n").unwrap();
    fs::write(
        dir.path().join("src/legacy/old.rs"),
        "#[path = \"mod.rs\"]\nmod legacy;\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let findings = finding_rows(&store);
    assert_eq!(
        file_reason(&findings, "src/legacy/mod.rs"),
        Some("detached module"),
        "{findings:?}"
    );
    assert_eq!(
        file_reason(&findings, "src/legacy/old.rs"),
        Some("detached module"),
        "{findings:?}"
    );
    assert_eq!(file_reason(&findings, "src/lib.rs"), None, "{findings:?}");
}

#[test]
fn a_markdown_file_beside_a_sealed_rust_package_is_not_unreachable() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    fs::write(dir.path().join("src/legacy.rs"), "fn old() {}\n").unwrap();
    fs::write(dir.path().join("README.md"), "# Title\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("README.md:unsupported"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(
        file_reason(&findings, "src/legacy.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "README.md"),
        "{findings:?}"
    );
}

#[test]
fn a_path_attribute_reaches_its_file_and_a_missing_target_withholds_the_default() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "#[path = \"other.rs\"]\nmod renamed;\n#[path = \"missing.rs\"]\nmod util;\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn reached() {}\n").unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn orphan() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("src/util.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/other.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/lib.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(file_reason(&findings, "src/other.rs"), None, "{findings:?}");
    assert_eq!(file_reason(&findings, "src/util.rs"), None, "{findings:?}");
    assert!(unused(&findings, "src/util.rs", "orphan"), "{findings:?}");
}
