use super::report;
use crate::knowledge::sync::sync_source;
use crate::knowledge::{open_in_memory, KnowledgeStore};
use rusqlite::params;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, label: &str, dir: &Path) {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
}

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn analysis(store: &KnowledgeStore, label: &str) -> (String, i64, String, String) {
    store
        .conn()
        .query_row(
            "SELECT a.status, a.evidence_generation, a.fingerprint, a.gaps
             FROM knowledge_analysis a
             JOIN knowledge_sources s ON s.id = a.source_id
             WHERE s.label = ?1",
            params![label],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

fn finding_rows(store: &KnowledgeStore) -> Vec<(i64, String, String, Option<String>, String)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT f.id, f.kind, kf.rel_path, ks.name, f.reason
             FROM knowledge_findings f
             JOIN knowledge_files kf ON kf.id = f.file_id
             LEFT JOIN knowledge_symbols ks ON ks.id = f.symbol_id
             ORDER BY f.id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn resolutions(
    store: &KnowledgeStore,
    specifier: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT res.status, ks.name, target.rel_path
             FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             LEFT JOIN knowledge_symbols ks ON ks.id = res.target_symbol_id
             LEFT JOIN knowledge_files target ON target.id = ks.file_id
             WHERE kr.specifier = ?1
             ORDER BY res.status, target.rel_path, ks.name",
        )
        .unwrap();
    statement
        .query_map(params![specifier], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn mutual_imports_are_unreachable_and_a_module_import_does_not_use_functions() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "mod b;\npub fn only_a() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "mod a;\npub fn only_b() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, _) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    let findings = finding_rows(&store);
    assert!(
        findings.iter().any(|(_, kind, path, symbol, reason)| {
            kind == "unreachable_file"
                && path == "a.rs"
                && symbol.is_none()
                && reason == "not reached from program or library_root"
        }),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|(_, kind, path, _, reason)| {
            kind == "unreachable_file"
                && path == "b.rs"
                && reason == "not reached from program or library_root"
        }),
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
    register(&store, "demo", dir.path());
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
    register(&store, "demo", dir.path());
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
fn a_file_reached_only_from_a_test_entry_says_so() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "#[test]\nfn checks_helper() { helper(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("util.rs"), "pub fn helper() {}\n").unwrap();
    // The test file does not import util.rs, so give the test a module reference.
    fs::write(
        dir.path().join("lib.rs"),
        "mod util;\n#[test]\nfn checks_helper() { helper(); }\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let findings = finding_rows(&store);
    assert!(
        findings.iter().any(|(_, kind, path, _, reason)| {
            kind == "unreachable_file"
                && path == "util.rs"
                && reason.starts_with("reachable only from test entries")
                && reason.contains("lib.rs")
        }),
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

#[test]
fn the_reader_returns_the_analysis_row_and_hides_findings_that_do_not_match_it() {
    let store = make_store();
    let dir = tempdir().unwrap();
    // Private, so the symbol is a finding. `pub` on neither root is withheld.
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let text = report(&store, None).unwrap();
    assert!(
        text.starts_with("source: demo\nstatus: complete\n"),
        "{text}"
    );
    assert!(text.contains("fingerprint: "), "{text}");
    assert!(text.contains("possibly_unused_symbol"), "{text}");

    store
        .conn()
        .execute("UPDATE knowledge_findings SET fingerprint = 'stale'", [])
        .unwrap();
    let hidden = report(&store, Some("demo")).unwrap();
    assert!(hidden.contains("status: complete"), "{hidden}");
    assert!(
        hidden.contains("findings:\nnone") || hidden.ends_with("findings:\nnone"),
        "{hidden}"
    );
    assert!(!hidden.contains("possibly_unused_symbol"), "{hidden}");
}

#[test]
fn a_source_with_no_analysis_row_does_not_invent_a_fingerprint() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    register(&store, "demo", dir.path());

    let text = report(&store, None).unwrap();
    assert_eq!(text, "source: demo\nno analysis stored\n");
    assert!(!text.contains("fingerprint"));
}

#[test]
fn several_sources_and_no_label_names_the_labels() {
    let store = make_store();
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    fs::write(first.path().join("a.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(second.path().join("b.rs"), "pub fn beta() {}\n").unwrap();
    register(&store, "alpha", first.path());
    register(&store, "beta", second.path());

    let error = report(&store, None).unwrap_err().to_string();
    assert!(error.contains("alpha"), "{error}");
    assert!(error.contains("beta"), "{error}");
    assert!(error.contains("source label is required"), "{error}");
}

fn unused(
    findings: &[(i64, String, String, Option<String>, String)],
    path: &str,
    name: &str,
) -> bool {
    findings.iter().any(|(_, kind, found, symbol, reason)| {
        kind == "possibly_unused_symbol"
            && found == path
            && symbol.as_deref() == Some(name)
            && reason == "no reference in sealed scope"
    })
}

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

#[test]
fn a_type_path_uses_that_types_method_and_not_a_free_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod other;\nstruct Foo;\nimpl Foo { fn helper() {} }\nfn main() { Foo::helper(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn helper() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers.iter().any(|(status, symbol, path)| {
            status == "resolved"
                && symbol.as_deref() == Some("helper")
                && path.as_deref() == Some("src/main.rs")
        }),
        "{helpers:?}"
    );
    assert!(
        helpers
            .iter()
            .all(|(_, _, path)| path.as_deref() != Some("src/other.rs")),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/other.rs", "helper"), "{findings:?}");
    assert!(!unused(&findings, "src/main.rs", "helper"), "{findings:?}");
}

#[test]
fn an_unknown_receiver_does_not_use_a_free_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "struct Foo;\nimpl Foo { fn helper(&self) {} }\nfn helper() {}\nfn main() { Self::helper(); }\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers.iter().any(|(status, _, path)| {
            status == "ambiguous" && path.as_deref() == Some("lib.rs")
        }),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(unused(&findings, "lib.rs", "helper"), "{findings:?}");
}

#[test]
fn an_inline_module_shares_the_file_and_records_the_limitation() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "mod inline { fn hidden() {} }\nfn main() { hidden(); }\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (_, _, _, gaps) = analysis(&store, "demo");
    assert!(gaps.contains("limitation:inline_modules"), "{gaps}");
    assert!(
        !unused(&finding_rows(&store), "main.rs", "hidden"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_macro_records_a_limitation_and_still_publishes_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "macro_rules! demo { () => {}; }\nfn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert!(gaps.contains("limitation:macros_not_expanded"), "{gaps}");
    assert!(
        unused(&finding_rows(&store), "main.rs", "kept"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn two_imports_of_the_same_symbol_are_ambiguous() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nuse crate::a::helper as h;\nuse crate::a::helper as h;\nfn main() { h(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/a.rs"), "pub(crate) fn helper() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let names = resolutions(&store, "h");
    assert!(
        names.iter().any(|(status, path_symbol, path)| {
            status == "ambiguous"
                && path_symbol.as_deref() == Some("helper")
                && path.as_deref() == Some("src/a.rs")
        }),
        "{names:?}"
    );
    assert!(!unused(&finding_rows(&store), "src/a.rs", "helper"));
}

#[test]
fn a_type_method_does_not_use_another_types_method_of_the_same_name() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod a;\nmod b;\nuse a::Foo;\nfn main() { Foo::helper(); }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/a.rs"),
        "pub struct Foo;\nimpl Foo { pub fn helper() {} }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/b.rs"),
        "pub struct Foo;\nimpl Foo { pub fn helper() {} }\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let helpers = resolutions(&store, "helper");
    assert!(
        helpers
            .iter()
            .any(|(_, _, path)| path.as_deref() == Some("src/a.rs")),
        "{helpers:?}"
    );
    assert!(
        helpers
            .iter()
            .all(|(_, _, path)| path.as_deref() != Some("src/b.rs")),
        "{helpers:?}"
    );
    let findings = finding_rows(&store);
    assert!(!unused(&findings, "src/a.rs", "helper"), "{findings:?}");
    assert!(unused(&findings, "src/b.rs", "helper"), "{findings:?}");
}

#[test]
fn an_external_path_does_not_use_a_method_in_scope() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn foo() {}\nstruct Bar;\nimpl Bar { fn method(&self) {} }\nfn main() { ::foo::method(); }\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let methods = resolutions(&store, "method");
    assert!(
        methods
            .iter()
            .all(|(status, symbol, _)| status == "unresolved" && symbol.is_none()),
        "{methods:?}"
    );
    assert!(
        unused(&finding_rows(&store), "lib.rs", "method"),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_parse_error_in_a_sibling_file_keeps_a_sealed_private_function() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/other.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/other.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/util.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/main.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/util.rs", "hidden"), "{findings:?}");
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/other.rs"),
        "{findings:?}"
    );
    let text = report(&store, Some("demo")).unwrap();
    assert!(
        text.contains("possibly_unused_symbol src/util.rs hidden no reference in sealed scope"),
        "{text}"
    );
    assert!(text.contains("src/other.rs:unsealed"), "{text}");
}

#[test]
fn a_parse_error_in_the_parent_withholds_the_child_without_a_stored_mod_edge() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() {}\nfn broken( {\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let module_edges: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_references kr
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = 'src/main.rs' AND kr.kind = 'module'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(module_edges, 0);
    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "incomplete");
    assert!(gaps.contains("src/main.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/main.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/util.rs:unsealed"), "{gaps}");
    assert!(
        finding_rows(&store).is_empty(),
        "{:?}",
        finding_rows(&store)
    );
}

#[test]
fn a_broken_module_file_unseals_its_directory_and_not_a_sibling() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/foo/nested")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "mod util;\nfn main() {}\n").unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn kept() {}\n").unwrap();
    fs::write(dir.path().join("src/foo.rs"), "fn broken( {\n").unwrap();
    fs::write(
        dir.path().join("src/foo/nested/deep.rs"),
        "fn hidden() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/foo.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("src/foo.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/foo/nested/deep.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/util.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/util.rs", "kept"), "{findings:?}");
    assert!(
        !unused(&findings, "src/foo/nested/deep.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/foo.rs" && path != "src/foo/nested/deep.rs"),
        "{findings:?}"
    );
}

#[test]
fn a_broken_parent_unseals_a_child_module_file_and_leaves_another_root_sealed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/foo")).unwrap();
    fs::create_dir_all(dir.path().join("src/bin")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn broken( {\n").unwrap();
    fs::write(dir.path().join("src/foo/mod.rs"), "fn hidden() {}\n").unwrap();
    fs::write(
        dir.path().join("src/bin/tool.rs"),
        "fn main() {}\nfn kept() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("src/main.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("src/foo/mod.rs:unsealed"), "{gaps}");
    assert!(!gaps.contains("src/bin/tool.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(unused(&findings, "src/bin/tool.rs", "kept"), "{findings:?}");
    assert!(
        !unused(&findings, "src/foo/mod.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "src/main.rs"),
        "{findings:?}"
    );
}

#[test]
fn a_parse_error_in_one_package_withholds_pub_crate_and_spares_the_other_package() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("crates/a/src")).unwrap();
    fs::create_dir_all(dir.path().join("crates/b/src")).unwrap();
    fs::write(
        dir.path().join("crates/a/Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("crates/b/Cargo.toml"),
        "[package]\nname = \"b\"\nversion = \"0.1.0\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("crates/a/src/lib.rs"),
        "pub(crate) fn hidden() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("crates/a/src/other.rs"), "fn broken( {\n").unwrap();
    fs::write(
        dir.path().join("crates/b/src/lib.rs"),
        "pub(crate) fn hidden() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert!(gaps.contains("crates/a/src/other.rs:unsealed"), "{gaps}");
    let findings = finding_rows(&store);
    assert!(
        unused(&findings, "crates/b/src/lib.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "crates/a/src/lib.rs", "hidden"),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .all(|(_, _, path, _, _)| path != "crates/a/src/other.rs"),
        "{findings:?}"
    );
}

#[test]
fn every_rust_file_failing_to_parse_is_incomplete_and_has_no_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "fn broken( {\n").unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "incomplete");
    assert!(gaps.contains("a.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("b.rs:unsealed"), "{gaps}");
    assert!(finding_rows(&store).is_empty());
    let text = report(&store, Some("demo")).unwrap();
    assert!(!text.contains("findings:"), "{text}");
}

#[test]
fn tampering_a_partial_fingerprint_hides_finding_lines_and_keeps_the_header() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn broken( {\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    store
        .conn()
        .execute("UPDATE knowledge_analysis SET fingerprint = 'tampered'", [])
        .unwrap();
    let hidden = report(&store, Some("demo")).unwrap();
    assert!(
        hidden.starts_with("source: demo\nstatus: partial\n"),
        "{hidden}"
    );
    assert!(hidden.contains("fingerprint: tampered"), "{hidden}");
    assert!(hidden.contains("src/other.rs:unsealed"), "{hidden}");
    assert!(
        hidden.contains("findings:\nnone") || hidden.ends_with("findings:\nnone"),
        "{hidden}"
    );
    assert!(!hidden.contains("possibly_unused_symbol"), "{hidden}");
}

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
    register(&store, "demo", dir.path());
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
    register(&store, "demo", dir.path());
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
