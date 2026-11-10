use super::super::{open_in_memory, KnowledgeStore};
use crate::knowledge::findings::report;
use crate::knowledge::sync::sync_source;
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
    assert!(gaps.contains("broken.rs:parse_error"), "{gaps}");
    assert!(gaps.contains("broken.rs:unsealed"), "{gaps}");
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_findings"), 0);
    let text = report(&store, Some("demo")).unwrap();
    assert!(text.contains("status: incomplete"), "{text}");
    assert!(!text.contains("findings:"), "{text}");
    assert!(!text.contains("possibly_unused_symbol"), "{text}");
    assert!(!text.contains("unreachable_file"), "{text}");
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
