use super::{analysis, finding_rows, resolutions, sync};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn mutual_imports_without_a_package_record_no_entry_points() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "mod b;\npub fn only_a() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "mod a;\npub fn only_b() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(
        gaps.split('\n').any(|line| line == "no_entry_points"),
        "{gaps}"
    );
    let findings = finding_rows(&store);
    assert!(
        !findings
            .iter()
            .any(|(_, kind, _, _, _)| kind == "unreachable_file"),
        "{findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|(_, kind, _, _, _)| kind == "possibly_unused_symbol"),
        "pub items on neither tree are withheld: {findings:?}"
    );
}

#[test]
fn a_module_import_reaches_the_file_without_marking_its_functions_used() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "mod util;\nfn main() {}\n").unwrap();
    fs::write(dir.path().join("src/util.rs"), "pub fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let findings = finding_rows(&store);
    assert!(
        findings.iter().any(|(_, kind, path, symbol, reason)| {
            kind == "possibly_unused_symbol"
                && path == "src/util.rs"
                && symbol.as_deref() == Some("helper")
                && reason == "no reference in sealed scope"
        }),
        "{findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|(_, kind, path, _, _)| kind == "unreachable_file" && path == "src/util.rs"),
        "{findings:?}"
    );
}

#[test]
fn an_unimported_call_resolves_to_neither_private_helper_and_a_called_function_stays_used() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    helper();\n    foo();\n    missing();\n}\nfn foo() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.rs"), "fn helper() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "fn helper() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let ambiguous: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             WHERE kr.specifier = 'helper' AND res.status = 'ambiguous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ambiguous, 0, "main.rs imports neither helper");
    let helper_rows = resolutions(&store, "helper");
    assert!(
        helper_rows
            .iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{helper_rows:?}"
    );
    let missing: String = store
        .conn()
        .query_row(
            "SELECT res.status FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             WHERE kr.specifier = 'missing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(missing, "unresolved");

    let findings = finding_rows(&store);
    for path in ["a.rs", "b.rs"] {
        assert!(
            findings.iter().any(|(_, kind, found, symbol, reason)| {
                kind == "possibly_unused_symbol"
                    && found == path
                    && symbol.as_deref() == Some("helper")
                    && reason == "no reference in sealed scope"
            }),
            "{path} helper should be unused: {findings:?}"
        );
    }
    assert!(
        !findings
            .iter()
            .any(|(_, kind, _, symbol, _)| kind == "possibly_unused_symbol"
                && symbol.as_deref() == Some("foo")),
        "{findings:?}"
    );
}

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
    // The library root is a membership root elsewhere. It does not declare util.rs.
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
