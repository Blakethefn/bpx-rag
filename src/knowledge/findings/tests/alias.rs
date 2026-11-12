use super::{analysis, entries, file_reason, finding_rows, resolutions, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_cfg_gated_mod_still_reaches_the_file() {
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
        "#[cfg(feature = \"missing\")]\nmod util;\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "Cargo.toml:unsupported");
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(file_reason(&findings, "src/util.rs"), None, "{findings:?}");
    assert!(unused(&findings, "src/util.rs", "hidden"), "{findings:?}");
}

#[test]
fn a_module_alias_leaves_the_called_function_unused() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nuse crate::a as other;\nfn main() { other::helper(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "");
    let helper = resolutions(&store, "helper");
    assert!(
        helper
            .iter()
            .all(|(found, symbol, _)| found == "unresolved" && symbol.is_none()),
        "{helper:?}"
    );
    assert!(!helper.is_empty(), "{helper:?}");
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(unused(&findings, "src/a.rs", "helper"), "{findings:?}");
}

#[test]
fn a_doc_comment_between_the_test_attribute_and_the_function_reports_it() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() {}\n#[test]\n/// documents the check\nfn checks() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(gaps, "");
    let test_entries = entries(&store)
        .into_iter()
        .filter(|(kind, reason, path)| {
            kind == "test" && path == "src/main.rs" && reason.contains("checks")
        })
        .count();
    assert_eq!(test_entries, 0);
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(unused(&findings, "src/main.rs", "checks"), "{findings:?}");
}
