use super::super::{open_in_memory, KnowledgeStore};
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
