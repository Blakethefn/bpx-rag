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
        findings.iter().any(|(_, kind, path, symbol, reason)| {
            kind == "possibly_unused_symbol"
                && path == "a.rs"
                && symbol.as_deref() == Some("only_a")
                && reason == "no indexed reference"
        }),
        "{findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|(_, kind, _, symbol, _)| kind == "possibly_unused_symbol"
                && symbol.as_deref() == Some("only_b")),
        "{findings:?}"
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
                && reason == "no indexed reference"
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
fn an_ambiguous_name_marks_every_candidate_and_an_unresolved_name_does_not_make_a_resolved_symbol_unused(
) {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    helper();\n    foo();\n    missing();\n}\nfn foo() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn helper() {}\n").unwrap();
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
    assert_eq!(ambiguous, 2, "both helper symbols are candidates");
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
    assert!(
        !findings
            .iter()
            .any(|(_, kind, _, symbol, _)| kind == "possibly_unused_symbol"
                && symbol.as_deref() == Some("helper")),
        "{findings:?}"
    );
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
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() {}\n").unwrap();
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
