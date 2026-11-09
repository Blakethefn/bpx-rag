//! Invalid UTF-8 files are skipped with a reason. Unreadable files are covered in `unreadable`.

use super::super::*;
use tempfile::tempdir;

fn make_store() -> KnowledgeStore {
    crate::knowledge::open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source("utf8", &dir.to_string_lossy(), &[], &[])
        .unwrap();
    store
        .get_source("utf8")
        .unwrap()
        .expect("registered source")
}

fn indexed_paths(store: &KnowledgeStore) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files ORDER BY rel_path")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn chunk_count_for(store: &KnowledgeStore, rel_path: &str) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta m
             JOIN knowledge_files f ON f.id = m.file_id
             WHERE f.rel_path = ?1",
            [rel_path],
            |row| row.get(0),
        )
        .unwrap()
}

/// Latin-1 text: `é` is the single byte 0xE9, which is not valid UTF-8 on its own.
const LATIN1: &[u8] = b"caf\xe9 menu: espresso and tea\n";

#[test]
fn invalid_utf8_file_is_skipped_with_a_reason_and_other_files_still_sync() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.rs"), "fn alpha_one() -> u8 { 1 }\n").unwrap();
    fs::write(dir.path().join("menu.txt"), LATIN1).unwrap();
    let source = register(&store, dir.path());

    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(
        result.files_added, 1,
        "the valid file must still be indexed"
    );
    assert_eq!(indexed_paths(&store), vec!["alpha.rs".to_string()]);
    assert_eq!(result.skipped_files.len(), 1);
    assert_eq!(result.skipped_files[0].rel_path, "menu.txt");
    assert!(
        result.skipped_files[0]
            .reason
            .starts_with("not valid UTF-8"),
        "reason was: {}",
        result.skipped_files[0].reason
    );
}

#[test]
fn file_that_becomes_invalid_loses_its_rows_and_is_reported() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), "alpha beta gamma\n").unwrap();
    let source = register(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    assert_eq!(indexed_paths(&store), vec!["notes.txt".to_string()]);

    fs::write(dir.path().join("notes.txt"), LATIN1).unwrap();
    let result = sync_source(&store, &source, None).unwrap();

    assert!(indexed_paths(&store).is_empty(), "stale rows survived");
    assert_eq!(chunk_count_for(&store, "notes.txt"), 0);
    assert_eq!(result.files_removed, 1);
    assert_eq!(result.skipped_files[0].rel_path, "notes.txt");
}

#[test]
fn file_is_indexed_again_once_its_content_is_valid() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), LATIN1).unwrap();
    let source = register(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    assert!(indexed_paths(&store).is_empty());

    fs::write(dir.path().join("notes.txt"), "alpha beta gamma\n").unwrap();
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(indexed_paths(&store), vec!["notes.txt".to_string()]);
    assert!(result.skipped_files.is_empty());
    assert!(chunk_count_for(&store, "notes.txt") > 0);
}

#[test]
fn event_driven_sync_skips_then_recovers_the_same_way() {
    // Watch mode reindexes only the paths that events name, so it needs the same behavior.
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), "alpha beta gamma\n").unwrap();
    let source = register(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    fs::write(dir.path().join("notes.txt"), LATIN1).unwrap();
    let skipped = sync_paths(&store, &source, None, &["notes.txt".to_string()]).unwrap();
    assert_eq!(skipped.skipped_files[0].rel_path, "notes.txt");
    assert!(indexed_paths(&store).is_empty());

    fs::write(dir.path().join("notes.txt"), "alpha beta gamma\n").unwrap();
    let recovered = sync_paths(&store, &source, None, &["notes.txt".to_string()]).unwrap();
    assert!(recovered.skipped_files.is_empty());
    assert_eq!(indexed_paths(&store), vec!["notes.txt".to_string()]);
}
