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
fn an_example_and_a_bench_are_program_roots_and_a_test_child_keeps_pub() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("examples/demo")).unwrap();
    fs::create_dir_all(dir.path().join("benches")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("examples/demo.rs"),
        "mod helper;\n#[test]\nfn checks() {}\nfn idle() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("examples/demo/helper.rs"),
        "// reached from the example\n",
    )
    .unwrap();
    fs::write(dir.path().join("benches/work.rs"), "fn bench_idle() {}\n").unwrap();
    fs::write(dir.path().join("tests/check.rs"), "mod util;\n").unwrap();
    fs::write(dir.path().join("tests/util.rs"), "pub fn exported() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported");
    let roots = entries(&store);
    assert!(
        has_entry(
            &roots,
            "program",
            "Cargo.toml implicit example",
            "examples/demo.rs"
        ),
        "{roots:?}"
    );
    assert!(
        has_entry(
            &roots,
            "program",
            "Cargo.toml implicit bench",
            "benches/work.rs"
        ),
        "{roots:?}"
    );
    assert!(
        has_entry(&roots, "test", "Cargo.toml implicit test", "tests/check.rs"),
        "{roots:?}"
    );
    assert!(
        has_entry(&roots, "test", "Cargo.toml implicit test", "tests/util.rs"),
        "{roots:?}"
    );
    assert!(
        !has_entry(
            &roots,
            "program",
            "Cargo.toml implicit example",
            "examples/demo/helper.rs"
        ),
        "{roots:?}"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        unused(&findings, "examples/demo.rs", "idle"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "examples/demo.rs", "checks"),
        "{findings:?}"
    );
    assert!(
        unused(&findings, "benches/work.rs", "bench_idle"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "tests/util.rs", "exported"),
        "{findings:?}"
    );
    for path in [
        "examples/demo.rs",
        "examples/demo/helper.rs",
        "benches/work.rs",
        "tests/check.rs",
        "tests/util.rs",
    ] {
        assert_eq!(file_reason(&findings, path), None, "{path}: {findings:?}");
    }
}

#[test]
fn a_test_root_alone_names_an_undeclared_file_and_skips_no_entry_points() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("tests/check.rs"), "// integration test\n").unwrap();
    fs::write(dir.path().join("src/legacy.rs"), "fn old() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported");
    let roots = entries(&store);
    assert!(
        has_entry(&roots, "test", "Cargo.toml implicit test", "tests/check.rs"),
        "{roots:?}"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "src/legacy.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert_eq!(
        file_reason(&findings, "tests/check.rs"),
        None,
        "{findings:?}"
    );
    assert!(unused(&findings, "src/legacy.rs", "old"), "{findings:?}");
}

#[test]
fn an_explicit_lib_path_roots_that_file_and_leaves_src_lib_rs_undeclared() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/internal.rs\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/internal.rs"), "pub fn exported() {}\n").unwrap();
    fs::write(dir.path().join("src/lib.rs"), "fn leftover() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported\nlimitation:public_api");
    let roots = entries(&store);
    assert!(
        has_entry(&roots, "library_root", "Cargo.toml lib", "src/internal.rs"),
        "{roots:?}"
    );
    assert!(
        !roots.iter().any(|(_, _, path)| path == "src/lib.rs"),
        "{roots:?}"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(
        file_reason(&findings, "src/internal.rs"),
        None,
        "{findings:?}"
    );
    assert_eq!(
        file_reason(&findings, "src/lib.rs"),
        Some("not declared by a module"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "src/internal.rs", "exported"),
        "{findings:?}"
    );
    assert!(unused(&findings, "src/lib.rs", "leftover"), "{findings:?}");
}
