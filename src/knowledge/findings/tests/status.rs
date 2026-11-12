use super::{analysis, finding_rows, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn an_unsupported_file_and_a_missing_entry_do_not_unseal_a_rust_file() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"missing\"\npath = \"src/missing.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("README.md"), "# Title\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("README.md:unsupported"), "{gaps}");
    assert!(gaps.contains("src/missing.rs:missing"), "{gaps}");
    assert!(!gaps.contains("unsealed"), "{gaps}");
    assert!(
        unused(&finding_rows(&store), "src/main.rs", "kept"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_stored_too_large_status_unseals_like_a_parse_error() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn orphan() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    store
        .conn()
        .execute(
            "UPDATE knowledge_files
             SET symbol_status = 'too_large', reference_status = 'too_large'
             WHERE rel_path = 'src/other.rs'",
            [],
        )
        .unwrap();
    let source_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_sources WHERE label = 'demo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    crate::knowledge::rebuild::rebuild_source(store.conn(), source_id, true).unwrap();

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/other.rs:too_large"), "{gaps}");
    assert!(gaps.contains("src/other.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/util.rs", "hidden"), "{findings:?}");
    assert!(!unused(&findings, "src/other.rs", "orphan"), "{findings:?}");
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/other.rs"),
        "{findings:?}"
    );
}
