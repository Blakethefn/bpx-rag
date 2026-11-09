use super::report;
use crate::knowledge::sync::{sync_paths, sync_source};
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

fn status_of(store: &KnowledgeStore, rel_path: &str) -> (String, String) {
    store
        .conn()
        .query_row(
            "SELECT symbol_status, reference_status FROM knowledge_files WHERE rel_path = ?1",
            params![rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn names(store: &KnowledgeStore, rel_path: &str) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT ks.name FROM knowledge_symbols ks
             JOIN knowledge_files kf ON kf.id = ks.file_id
             WHERE kf.rel_path = ?1 ORDER BY ks.name",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn reference_specifiers(store: &KnowledgeStore, rel_path: &str) -> Vec<(String, String)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT kr.kind, kr.specifier FROM knowledge_references kr
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = ?1 ORDER BY kr.kind, kr.specifier",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
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

fn count(store: &KnowledgeStore, sql: &str) -> i64 {
    store.conn().query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn editing_a_file_replaces_its_symbols_references_and_status() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() { other(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    assert_eq!(names(&store, "lib.rs"), vec!["alpha".to_string()]);
    assert_eq!(
        reference_specifiers(&store, "lib.rs"),
        vec![("name".to_string(), "other".to_string())]
    );
    assert_eq!(
        status_of(&store, "lib.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    let previous_symbol: i64 = store
        .conn()
        .query_row("SELECT id FROM knowledge_symbols", [], |row| row.get(0))
        .unwrap();

    fs::write(dir.path().join("lib.rs"), "pub fn beta() { fresh(); }\n").unwrap();
    sync(&store, "demo");

    assert_eq!(names(&store, "lib.rs"), vec!["beta".to_string()]);
    assert_eq!(
        reference_specifiers(&store, "lib.rs"),
        vec![("name".to_string(), "fresh".to_string())]
    );
    assert_eq!(
        status_of(&store, "lib.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    let still_there: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_symbols WHERE id = ?1",
            params![previous_symbol],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_there, 0, "the previous symbol row must be gone");
}

#[test]
fn deleting_a_file_drops_its_evidence_and_nulls_an_entry_that_pointed_at_it() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"demo\"\npath = \"src/main.rs\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let target_before: Option<i64> = store
        .conn()
        .query_row(
            "SELECT target_file_id FROM knowledge_entries WHERE target_rel_path = 'src/main.rs' AND reason = 'Cargo.toml bin'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        target_before.is_some(),
        "the manifest entry resolves while the file is indexed"
    );

    fs::remove_file(dir.path().join("src/main.rs")).unwrap();
    sync(&store, "demo");

    assert!(names(&store, "src/main.rs").is_empty());
    assert!(reference_specifiers(&store, "src/main.rs").is_empty());
    assert_eq!(names(&store, "src/keep.rs"), vec!["keep".to_string()]);
    let (target_path, target_id, evidence_path): (String, Option<i64>, String) = store
        .conn()
        .query_row(
            "SELECT e.target_rel_path, e.target_file_id, kf.rel_path
             FROM knowledge_entries e
             JOIN knowledge_files kf ON kf.id = e.evidence_file_id
             WHERE e.reason = 'Cargo.toml bin'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(target_path, "src/main.rs");
    assert_eq!(target_id, None);
    assert_eq!(evidence_path, "Cargo.toml");
    let main_entries: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_entries e
             JOIN knowledge_files kf ON kf.id = e.evidence_file_id
             WHERE kf.rel_path = 'src/main.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        main_entries, 0,
        "evidence that lived on the deleted file is gone"
    );
}

#[test]
fn removing_a_source_drops_evidence_resolutions_analysis_and_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_references") > 0);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_resolutions") > 0);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_analysis"), 1);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_findings") > 0);

    store.remove_source("demo").unwrap();

    for table in [
        "knowledge_references",
        "knowledge_entries",
        "knowledge_resolutions",
        "knowledge_analysis",
        "knowledge_findings",
        "knowledge_symbols",
    ] {
        assert_eq!(
            count(&store, &format!("SELECT COUNT(*) FROM {table}")),
            0,
            "{table} still has rows"
        );
    }
}

#[test]
fn a_parse_error_publishes_an_incomplete_analysis_and_no_conclusions() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("broken.rs"),
        "pub fn fine() {}\nfn broken( {\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    let chunks: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta kcm
             JOIN knowledge_files kf ON kf.id = kcm.file_id
             WHERE kf.rel_path = 'broken.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(chunks > 0, "parse error must keep the file's chunks");
    assert_eq!(
        status_of(&store, "broken.rs"),
        ("parse_error".to_string(), "parse_error".to_string())
    );
    assert!(names(&store, "broken.rs").is_empty());
    assert!(reference_specifiers(&store, "broken.rs").is_empty());
    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "incomplete");
    assert!(gaps.contains("broken.rs"), "{gaps}");
    assert!(gaps.contains("parse_error"), "{gaps}");
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_findings"), 0);
    let text = report(&store, Some("demo")).unwrap();
    assert!(text.contains("status: incomplete"), "{text}");
    assert!(!text.contains("possibly_unused_symbol"), "{text}");
    assert!(!text.contains("unreachable_file"), "{text}");
}

#[test]
fn a_file_that_declares_nothing_stores_complete_and_no_symbol_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("quiet.rs"), "// no declarations\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    assert_eq!(
        status_of(&store, "quiet.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    assert!(names(&store, "quiet.rs").is_empty());
    let stamped: String = store
        .conn()
        .query_row(
            "SELECT CASE WHEN symbols_hash = content_hash AND references_hash = content_hash
                         THEN 'stamped' ELSE 'unstamped' END
             FROM knowledge_files WHERE rel_path = 'quiet.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stamped, "stamped");
}

#[test]
fn a_partial_sync_does_not_rebuild_another_files_references_from_disk() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "mod b;\npub fn caller() { kept(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn kept() { other(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    let before = reference_specifiers(&store, "b.rs");
    assert!(
        before.iter().any(|(_, specifier)| specifier == "other"),
        "{before:?}"
    );

    fs::write(
        dir.path().join("b.rs"),
        "pub fn kept() { other(); }\npub fn added() { fresh_name(); }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "mod b;\npub fn caller() { kept(); } // edited\n",
    )
    .unwrap();
    let source = store.get_source("demo").unwrap().unwrap();
    sync_paths(&store, &source, None, &["a.rs".to_string()]).unwrap();

    assert_eq!(reference_specifiers(&store, "b.rs"), before);
    assert!(!names(&store, "b.rs").contains(&"added".to_string()));
    let other_status: String = store
        .conn()
        .query_row(
            "SELECT res.status FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = 'b.rs' AND kr.specifier = 'other'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(other_status, "unresolved");
}

#[test]
fn a_symbol_version_backfill_rebuilds_resolutions_and_the_analysis_row() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() { beta(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    let (_, generation, _, _) = analysis(&store, "demo");
    store
        .conn()
        .execute_batch(
            "DELETE FROM knowledge_symbols;
             DELETE FROM knowledge_resolutions;
             UPDATE knowledge_files SET symbol_version = 0;",
        )
        .unwrap();

    let source = store.get_source("demo").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();
    assert_eq!(result.files_updated, 0);
    assert_eq!(result.files_unchanged, 1);
    assert_eq!(names(&store, "lib.rs"), vec!["alpha".to_string()]);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_resolutions") > 0);
    let (_, next_generation, _, _) = analysis(&store, "demo");
    assert!(
        next_generation > generation,
        "backfill must rewrite the analysis row"
    );
}

#[test]
fn an_unchanged_sync_keeps_the_fingerprint_and_the_finding_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    )
    .unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    let (_, generation, fingerprint, _) = analysis(&store, "demo");
    let findings = finding_rows(&store);
    assert!(!findings.is_empty());

    sync(&store, "demo");

    let (_, next_generation, next_fingerprint, _) = analysis(&store, "demo");
    assert_eq!(next_fingerprint, fingerprint);
    assert_eq!(next_generation, generation);
    assert_eq!(finding_rows(&store), findings);
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
