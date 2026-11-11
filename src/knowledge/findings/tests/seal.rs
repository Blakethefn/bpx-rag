use super::super::report;
use super::{make_store, register, sync, analysis, finding_rows, unused};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_parse_error_in_a_sibling_file_keeps_a_sealed_private_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/other.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/other.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/util.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/main.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/util.rs", "hidden"), "{findings:?}");
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/other.rs"),
        "{findings:?}"
    );
    let text = report(&store, Some("demo")).unwrap();
    assert!(
        text.contains("possibly_unused_symbol src/util.rs hidden no reference in sealed scope"),
        "{text}"
    );
    assert!(text.contains("src/other.rs:unsealed"), "{text}");
}

#[test]
fn a_parse_error_in_the_parent_withholds_the_child_without_a_stored_mod_edge() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() {}\nfn broken( {\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let module_edges: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_references kr
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = 'src/main.rs' AND kr.kind = 'module'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(module_edges, 0);
    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "incomplete");
    assert!(gaps.contains("src/main.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/main.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/util.rs:unsealed"), "{gaps}");
    assert!(
        finding_rows(&store).is_empty(),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_broken_module_file_unseals_its_directory_and_not_a_sibling() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/foo/nested")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "mod util;\nfn main() {}\n").unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn kept() {}\n").unwrap();
    fs::write(dir.path().join("src/foo.rs"), "fn broken( {\n").unwrap();
    fs::write(
        dir.path().join("src/foo/nested/deep.rs"),
        "fn hidden() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/foo.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/foo.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/foo/nested/deep.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/util.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/util.rs", "kept"), "{findings:?}");
    assert!(
        !unused(&findings, "src/foo/nested/deep.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/foo.rs" && path != "src/foo/nested/deep.rs"),
        "{findings:?}"
    );
}

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
    register(&store, "demo", dir.path());
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
    register(&store, "demo", dir.path());
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
