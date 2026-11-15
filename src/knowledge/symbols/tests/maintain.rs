use super::super::*;
use super::{count, exact, hit, lookup_with, sync};
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

mod backfill;

#[test]
fn rename_on_resync_replaces_the_old_name() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn old_name() {}\n").unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");
    assert_eq!(exact(&store, "old_name").len(), 1);

    fs::write(dir.path().join("lib.rs"), "pub fn new_name() {}\n").unwrap();
    sync(&store, "test");

    assert!(exact(&store, "old_name").is_empty());
    assert_eq!(
        exact(&store, "new_name"),
        vec![hit("test", "lib.rs", "new_name", "function", None, (1, 1))]
    );
}

#[test]
fn moved_file_drops_the_old_path_and_indexes_the_new_one() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn moved() {}\n").unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");

    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::rename(dir.path().join("lib.rs"), dir.path().join("sub/lib.rs")).unwrap();
    sync(&store, "test");

    assert_eq!(
        exact(&store, "moved"),
        vec![hit("test", "sub/lib.rs", "moved", "function", None, (1, 1))]
    );
}

#[test]
fn removed_file_drops_its_symbols() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("keep.rs"), "pub fn keep() {}\n").unwrap();
    fs::write(dir.path().join("gone.rs"), "pub fn gone() {}\n").unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");

    fs::remove_file(dir.path().join("gone.rs")).unwrap();
    sync(&store, "test");

    assert!(exact(&store, "gone").is_empty());
    assert_eq!(exact(&store, "keep").len(), 1);
    let orphans: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_symbols ks
             LEFT JOIN knowledge_files kf ON kf.id = ks.file_id
             WHERE kf.id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);
}

#[test]
fn parse_error_file_keeps_its_chunks_and_reports_no_symbols() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("broken.rs"),
        "pub fn fine() {}\nfn broken( {\n",
    )
    .unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");

    assert!(exact(&store, "fine").is_empty());
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
    let report = lookup_with(&store, "fine", MatchMode::Exact, None, 20);
    assert!(
        report.pending.is_empty(),
        "a parse-error file is indexed, not pending"
    );
}
