use super::{
    make_store,
    register,
    sync,
    analysis,
    finding_rows,
    unused,
    file_reason,
    entries,
    has_entry,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn an_implicit_lib_and_bin_split_pub_items_and_keep_a_library_child() {
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
        "mod util;\npub fn library_item() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/util.rs"),
        "// reached from the library\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "pub fn bin_item() {}\nfn main() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported\nlimitation:public_api");
    let roots = entries(&store);
    assert!(
        has_entry(
            &roots,
            "library_root",
            "Cargo.toml implicit lib",
            "src/lib.rs"
        ),
        "{roots:?}"
    );
    assert!(
        has_entry(&roots, "program", "Cargo.toml implicit bin", "src/main.rs"),
        "{roots:?}"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        !unused(&findings, "src/lib.rs", "library_item"),
        "{findings:?}"
    );
    assert!(unused(&findings, "src/main.rs", "bin_item"), "{findings:?}");
    assert_eq!(file_reason(&findings, "src/util.rs"), None, "{findings:?}");
    assert_eq!(file_reason(&findings, "src/lib.rs"), None, "{findings:?}");
    assert_eq!(file_reason(&findings, "src/main.rs"), None, "{findings:?}");
}

#[test]
fn a_parse_error_keeps_binary_pub_and_a_sealed_private_item_and_withholds_pub_crate() {
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
        "mod util;\npub fn library_item() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "pub fn bin_item() {}\npub(crate) fn bin_crate() {}\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert_eq!(
        gaps,
        "Cargo.toml:unsupported\nlimitation:public_api\nsrc/other.rs:parse_error\nsrc/other.rs:unsealed"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(unused(&findings, "src/util.rs", "hidden"), "{findings:?}");
    assert!(unused(&findings, "src/main.rs", "bin_item"), "{findings:?}");
    assert!(
        !unused(&findings, "src/main.rs", "bin_crate"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "src/lib.rs", "library_item"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, kind, path, _, _)| kind != "unreachable_file" && path != "src/other.rs"),
        "{findings:?}"
    );
}

#[test]
fn pub_super_is_reported_and_pub_in_paths_follow_their_scope() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "mod util;\nmod other;\n").unwrap();
    fs::write(
        dir.path().join("src/util.rs"),
        "pub(super) fn parent_scoped() {}\npub(in super::other) fn sibling_scoped() {}\npub(in crate::util) fn crate_scoped() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "// sealed scope\n").unwrap();
    fs::write(dir.path().join("src/broken.rs"), "fn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert_eq!(
        gaps,
        "Cargo.toml:unsupported\nsrc/broken.rs:parse_error\nsrc/broken.rs:unsealed"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        unused(&findings, "src/util.rs", "parent_scoped"),
        "{findings:?}"
    );
    assert!(
        unused(&findings, "src/util.rs", "sibling_scoped"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "src/util.rs", "crate_scoped"),
        "{findings:?}"
    );
}
