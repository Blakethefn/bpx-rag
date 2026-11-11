use super::*;

#[test]
fn next_analysis_reads_current_filesystem_state() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.md"), "No links yet.\n").unwrap();
    fs::write(dir.path().join("beta.md"), "\n").unwrap();

    let before = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();
    fs::write(dir.path().join("alpha.md"), "[[beta]]\n").unwrap();
    let after = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert!(before.outgoing.is_empty());
    assert_eq!(after.outgoing.len(), 1);
}

#[test]
fn file_outside_root_is_rejected() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let target = outside.path().join("outside.md");
    fs::write(&target, "text\n").unwrap();

    assert!(analyze_file(&target, Some(dir.path())).is_err());
}

#[test]
fn ignored_directories_are_not_scanned_for_incoming_references() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
    fs::write(dir.path().join("src/alpha.md"), "# Alpha\n").unwrap();
    fs::write(
        dir.path().join("node_modules/pkg/ref.md"),
        "[[../../src/alpha]]\n",
    )
    .unwrap();

    let result = analyze_file(&dir.path().join("src/alpha.md"), Some(dir.path())).unwrap();

    assert!(result.incoming.is_empty());
}

#[test]
fn selected_ignored_file_still_returns_its_own_outgoing_links() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "alpha.md\n").unwrap();
    fs::write(dir.path().join("alpha.md"), "See [[beta]].\n").unwrap();
    fs::write(dir.path().join("beta.md"), "# Beta\n").unwrap();

    let result = analyze_file(&dir.path().join("alpha.md"), Some(dir.path())).unwrap();

    assert_eq!(result.outgoing.len(), 1);
    assert_eq!(result.outgoing[0].to.as_deref(), Some("beta.md"));
}
