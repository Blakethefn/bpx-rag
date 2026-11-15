use super::{analysis, entries, file_reason, finding_rows, has_entry, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

mod entry;

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
    add_source(&store, "demo", dir.path());
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
