use super::super::super::report;
use super::super::{analysis, finding_rows, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_broken_parent_unseals_a_child_module_file_and_leaves_another_root_sealed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/foo")).unwrap();
    fs::create_dir_all(dir.path().join("src/bin")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn broken( {\n").unwrap();
    fs::write(dir.path().join("src/foo/mod.rs"), "fn hidden() {}\n").unwrap();
    fs::write(
        dir.path().join("src/bin/tool.rs"),
        "fn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/main.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/foo/mod.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/bin/tool.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/bin/tool.rs", "kept"), "{findings:?}");
    assert!(
        !unused(&findings, "src/foo/mod.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/main.rs"),
        "{findings:?}"
    );
}

#[test]
fn a_parse_error_in_one_package_withholds_pub_crate_and_spares_the_other_package() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("crates/a/src")).unwrap();
    fs::create_dir_all(dir.path().join("crates/b/src")).unwrap();
    fs::write(
        dir.path().join("crates/a/Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("crates/b/Cargo.toml"),
        "[package]\nname = \"b\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("crates/a/src/lib.rs"),
        "pub(crate) fn hidden() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("crates/a/src/other.rs"), "fn broken( {\n").unwrap();
    fs::write(
        dir.path().join("crates/b/src/lib.rs"),
        "pub(crate) fn hidden() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("crates/a/src/other.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(
        unused(&findings, "crates/b/src/lib.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "crates/a/src/lib.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "crates/a/src/other.rs"),
        "{findings:?}"
    );
}
