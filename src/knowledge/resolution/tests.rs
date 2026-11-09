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

fn count(store: &KnowledgeStore, sql: &str) -> i64 {
    store.conn().query_row(sql, [], |row| row.get(0)).unwrap()
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
