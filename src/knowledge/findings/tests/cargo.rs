use super::{analysis, file_reason, finding_rows, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn autobins_false_omits_an_unrooted_bin_and_keeps_an_explicit_bin() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/bin")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nautobins = false\n\n[[bin]]\nname = \"kept\"\npath = \"src/kept.rs\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/kept.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("src/bin/tool.rs"), "fn orphan() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(!gaps.contains("no_entry_points"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(file_reason(&findings, "src/kept.rs"), None, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "src/bin/tool.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    let implicit: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_entries
             WHERE reason = 'Cargo.toml implicit bin' AND target_rel_path = 'src/bin/tool.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(implicit, 0);
}

#[test]
fn a_workspace_member_does_not_inherit_a_virtual_manifest() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("member/src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn not_a_root() {}\n").unwrap();
    fs::write(
        dir.path().join("member/Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("member/src/lib.rs"),
        "pub fn exported() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("member/src/legacy.rs"), "fn old() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(!gaps.contains("no_entry_points"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(file_reason(&findings, "src/lib.rs"), None, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "member/src/lib.rs"),
        None,
        "{findings:?}"
    );
    assert_eq!(
        file_reason(&findings, "member/src/legacy.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "member/src/lib.rs", "exported"),
        "{findings:?}"
    );
    let workspace_root: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_entries
             WHERE target_rel_path = 'src/lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(workspace_root, 0);
    let member_root: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_entries
             WHERE reason = 'Cargo.toml implicit lib' AND target_rel_path = 'member/src/lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(member_root, 1);
}

#[test]
fn a_bin_added_after_the_manifest_becomes_an_implicit_root() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    fs::create_dir(dir.path().join("src/bin")).unwrap();
    fs::write(dir.path().join("src/bin/tool.rs"), "fn orphan() {}\n").unwrap();
    sync(&store, "demo");

    let implicit: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_entries
             WHERE reason = 'Cargo.toml implicit bin'
               AND target_rel_path = 'src/bin/tool.rs'
               AND target_file_id IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(implicit, 1);
    assert_eq!(
        file_reason(&finding_rows(&store), "src/bin/tool.rs"),
        None,
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_parse_error_publishes_no_unreachable_file_for_an_undeclared_sibling() {
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
    fs::write(dir.path().join("src/broken.rs"), "fn broken( {\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/broken.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/legacy.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert_eq!(
        file_reason(&findings, "src/legacy.rs"),
        None,
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, kind, _, _, _)| kind != "unreachable_file"),
        "{findings:?}"
    );
    assert!(unused(&findings, "src/legacy.rs", "old"), "{findings:?}");
}
