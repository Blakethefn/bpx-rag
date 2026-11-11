use super::{
    make_store,
    register,
    sync,
    analysis,
    finding_rows,
    resolutions,
    unused,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn an_alias_uses_only_the_imported_helper() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nuse crate::a::helper as h;\nfn main() { h(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    fs::write(dir.path().join("src/b.rs"), "fn helper() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let targets: Vec<_> = resolutions(&store, "h")
        .into_iter()
        .filter(|(_, symbol, _)| symbol.is_some())
        .map(|(_, _, path)| path)
        .collect();
    assert!(
        targets
            .iter()
            .all(|path| path.as_deref() == Some("src/a.rs")),
        "{targets:?}"
    );
    assert!(!targets.is_empty(), "h must resolve to a's helper");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/b.rs", "helper"), "{findings:?}");
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
}

#[test]
fn a_private_function_is_not_used_by_a_call_in_another_module() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/b.rs"), "fn caller() { hidden(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let hidden = resolutions(&store, "hidden");
    assert!(
        hidden
            .iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{hidden:?}"
    );
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/a.rs", "hidden"), "{findings:?}");
}

#[test]
fn a_library_pub_item_is_withheld_and_pub_crate_can_be_unused() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn exported() {}\npub(crate) fn internal() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("limitation:public_api"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/lib.rs", "internal"), "{findings:?}");
    assert!(!unused(&findings, "src/lib.rs", "exported"), "{findings:?}");
    assert!(
        !findings
            .iter()
            .any(|(_, kind, path, _, _)| kind == "unreachable_file" && path == "src/lib.rs"),
        "{findings:?}"
    );
}

#[test]
fn a_parsed_non_rust_file_has_no_finding_and_an_unsupported_file_keeps_its_gap() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        dir.path().join("helper.py"),
        "def helper():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("app.js"),
        "function helper() { return 1; }\n",
    )
    .unwrap();
    fs::write(dir.path().join("README.md"), "# Title\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    let mut lines: Vec<&str> = gaps.split('\n').filter(|line| !line.is_empty()).collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        vec![
            "README.md:unsupported",
            "app.js:resolution_unsupported",
            "helper.py:resolution_unsupported",
        ]
    );
    let findings = finding_rows(&store);
    assert!(
        findings.iter().all(|(_, _, path, _, _)| {
            path != "helper.py" && path != "app.js" && path != "README.md"
        }),
        "{findings:?}"
    );
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_glob_uses_visible_items_and_an_unresolved_glob_does_not_blank_the_source() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nuse a::*;\nuse missing::*;\nfn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/a.rs"),
        "pub fn helper() {}\nfn hidden() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, _) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(unused(&findings, "src/a.rs", "hidden"), "{findings:?}");
    assert!(unused(&findings, "src/main.rs", "kept"), "{findings:?}");
}

#[test]
fn a_prelude_name_does_not_use_a_function_in_another_module() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod other;\nfn main() { let _ = Some; }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn Some() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let some = resolutions(&store, "Some");
    assert!(
        some.iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{some:?}"
    );
    assert!(
        unused(&finding_rows(&store), "src/other.rs", "Some"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn two_imports_of_one_local_name_are_ambiguous() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nuse crate::a::helper as h;\nuse crate::b::helper as h;\nfn main() { h(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    fs::write(dir.path().join("src/b.rs"), "pub(crate) fn helper() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let ambiguous: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             WHERE kr.specifier = 'h' AND kr.kind = 'name' AND res.status = 'ambiguous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ambiguous, 2);
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(!unused(&findings, "src/b.rs", "helper"), "{findings:?}");
}
