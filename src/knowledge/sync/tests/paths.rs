use super::super::*;
use crate::knowledge::open_in_memory;
use rusqlite::params;
use std::fs;
use tempfile::tempdir;

fn register(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source("test", dir.to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
    source
}

fn indexed_paths(store: &KnowledgeStore) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files ORDER BY rel_path")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn chunk_rowids(store: &KnowledgeStore, rel_path: &str) -> Vec<i64> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT kcm.chunk_rowid FROM knowledge_chunk_meta kcm
             JOIN knowledge_files kf ON kf.id = kcm.file_id
             WHERE kf.rel_path = ?1 ORDER BY kcm.chunk_rowid",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn symbol_names(store: &KnowledgeStore) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare("SELECT name FROM knowledge_symbols ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn source_of(store: &KnowledgeStore) -> KnowledgeSourceInfo {
    store.get_source("test").unwrap().unwrap()
}

#[test]
fn paths_add_modify_rename_and_remove_files() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    let source = register(&store, dir.path());

    fs::write(dir.path().join("b.rs"), "pub fn beta() -> u8 { 2 }\n").unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "pub fn alpha_renamed() -> u8 { 1 }\n",
    )
    .unwrap();
    sync_paths(
        &store,
        &source,
        None,
        &["b.rs".to_string(), "a.rs".to_string()],
    )
    .unwrap();
    assert_eq!(indexed_paths(&store), vec!["a.rs", "b.rs"]);
    assert_eq!(symbol_names(&store), vec!["alpha_renamed", "beta"]);

    fs::rename(dir.path().join("a.rs"), dir.path().join("c.rs")).unwrap();
    sync_paths(
        &store,
        &source,
        None,
        &["a.rs".to_string(), "c.rs".to_string()],
    )
    .unwrap();
    assert_eq!(indexed_paths(&store), vec!["b.rs", "c.rs"]);

    fs::remove_file(dir.path().join("b.rs")).unwrap();
    sync_paths(&store, &source, None, &["b.rs".to_string()]).unwrap();
    assert_eq!(indexed_paths(&store), vec!["c.rs"]);
    assert_eq!(symbol_names(&store), vec!["alpha_renamed"]);
}

#[test]
fn unchanged_content_rewrites_no_chunks_or_vectors() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn beta() -> u8 { 2 }\n").unwrap();
    let source = register(&store, dir.path());
    let before = chunk_rowids(&store, "a.rs");

    let result = sync_paths(&store, &source, None, &["a.rs".to_string()]).unwrap();
    assert_eq!(result.chunks_indexed, 0);
    assert_eq!(chunk_rowids(&store, "a.rs"), before);
}

#[test]
fn sensitive_content_removes_the_indexed_file() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("config.rs"),
        "pub const NAME: &str = \"demo\";\n",
    )
    .unwrap();
    let source = register(&store, dir.path());
    assert_eq!(indexed_paths(&store), vec!["config.rs"]);

    fs::write(
        dir.path().join("config.rs"),
        format!("pub const KEY: &str = \"AIza{}\";\n", "A".repeat(35)),
    )
    .unwrap();
    sync_paths(&store, &source, None, &["config.rs".to_string()]).unwrap();
    assert!(
        indexed_paths(&store).is_empty(),
        "sensitive files must not stay indexed"
    );
}

#[test]
fn unreadable_changed_file_is_skipped_and_other_files_stay_indexed() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn beta() -> u8 { 2 }\n").unwrap();
    let source = register(&store, dir.path());

    fs::write(dir.path().join("a.rs"), "pub fn alpha_v2() -> u8 { 1 }\n").unwrap();
    fs::set_permissions(
        dir.path().join("a.rs"),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    if fs::File::open(dir.path().join("a.rs")).is_ok() {
        eprintln!("skipping: permissions are not enforced for this user");
        return;
    }
    let result = sync_paths(
        &store,
        &source,
        None,
        &["a.rs".to_string(), "b.rs".to_string()],
    )
    .expect("an unreadable changed file is skipped, not an error");
    assert_eq!(result.skipped_files.len(), 1);
    assert_eq!(result.skipped_files[0].rel_path, "a.rs");
    assert!(result.skipped_files[0].reason.starts_with("unreadable: "));
    // The stale rows of the unreadable file are removed. The other file is untouched.
    assert_eq!(indexed_paths(&store), vec!["b.rs"]);
    assert_eq!(symbol_names(&store), vec!["beta"]);
}

#[test]
fn reindex_matches_a_full_sync_for_the_same_changes() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    let source = register(&store, dir.path());

    fs::write(
        dir.path().join("a.rs"),
        "pub fn alpha() -> u8 { 1 }\npub fn gamma() -> u8 { 3 }\n",
    )
    .unwrap();
    sync_paths(&store, &source, None, &["a.rs".to_string()]).unwrap();
    let partial_chunks = chunk_rowids(&store, "a.rs").len();

    let full = sync_source(&store, &source_of(&store), None).unwrap();
    assert_eq!(
        full.files_updated, 0,
        "a path reindex must leave nothing for a full sync"
    );
    assert_eq!(chunk_rowids(&store, "a.rs").len(), partial_chunks);
    assert_eq!(symbol_names(&store), vec!["alpha", "gamma"]);
}
