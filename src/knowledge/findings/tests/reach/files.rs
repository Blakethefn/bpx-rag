use super::super::{analysis, finding_rows, resolutions, sync};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_file_reached_only_from_a_test_root_is_not_an_unused_file() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "// crate root\n").unwrap();
    fs::write(
        dir.path().join("tests/checks.rs"),
        "#[path = \"util.rs\"]\nmod util;\n#[test]\nfn checks_helper() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("tests/util.rs"), "pub fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let findings = finding_rows(&store);
    assert!(
        !findings
            .iter()
            .any(|(_, kind, path, _, _)| kind == "unreachable_file" && path == "tests/util.rs"),
        "{findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|(_, kind, _, symbol, _)| kind == "possibly_unused_symbol"
                && symbol.as_deref() == Some("helper")),
        "{findings:?}"
    );
}
