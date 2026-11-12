use super::{analysis, entries, file_reason, finding_rows, has_entry, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn autotests_false_does_not_root_a_test_file_added_on_the_next_sync() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nautotests = false\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    fs::create_dir(dir.path().join("tests")).unwrap();
    fs::write(dir.path().join("tests/probe.rs"), "fn orphan() {}\n").unwrap();
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported");
    let roots = entries(&store);
    assert!(
        !roots.iter().any(|(_, reason, path)| {
            reason == "Cargo.toml implicit test" && path == "tests/probe.rs"
        }),
        "{roots:?}"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "tests/probe.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert!(
        unused(&findings, "tests/probe.rs", "orphan"),
        "{findings:?}"
    );
}

#[test]
fn deleting_an_implicit_bin_drops_its_entry_and_names_no_finding() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/bin")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    fs::write(dir.path().join("src/bin/tool.rs"), "fn orphan() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let before = entries(&store);
    assert!(
        has_entry(
            &before,
            "program",
            "Cargo.toml implicit bin",
            "src/bin/tool.rs"
        ),
        "{before:?}"
    );

    fs::remove_file(dir.path().join("src/bin/tool.rs")).unwrap();
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported");
    let after = entries(&store);
    assert!(
        !after.iter().any(|(_, _, path)| path == "src/bin/tool.rs"),
        "{after:?}"
    );
    let findings = finding_rows(&store);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_file_on_both_the_library_and_the_bin_withholds_pub_and_reports_private() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "mod shared;\n").unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod shared;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/shared.rs"),
        "pub fn exported() {}\nfn hidden() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported\nlimitation:public_api");
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "src/shared.rs"),
        None,
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "src/shared.rs", "exported"),
        "{findings:?}"
    );
    assert!(unused(&findings, "src/shared.rs", "hidden"), "{findings:?}");
}
