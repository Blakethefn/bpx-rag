//! A file that cannot be read is skipped with a reason, like invalid UTF-8, so one bad file
//! cannot stop the rest of its source from syncing. A failure of the directory walk itself
//! is not a per-file problem and still fails the sync.

use super::super::*;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

fn make_store() -> KnowledgeStore {
    crate::knowledge::open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source("unreadable", &dir.to_string_lossy(), &[], &[])
        .unwrap();
    store
        .get_source("unreadable")
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

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// Restores permissions when dropped, so a failed assertion cannot leave a locked file behind.
struct Restore(PathBuf);

impl Drop for Restore {
    fn drop(&mut self) {
        let mode = if self.0.is_dir() { 0o755 } else { 0o644 };
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(mode));
    }
}

/// Lock `path`. Returns `None` when permissions do not block reads, as when running as root.
fn lock(path: &Path) -> Option<Restore> {
    let guard = Restore(path.to_path_buf());
    set_mode(path, 0o000);
    let blocked = if path.is_dir() {
        fs::read_dir(path).is_err()
    } else {
        fs::read(path).is_err()
    };
    blocked.then_some(guard)
}

#[test]
fn unreadable_file_is_skipped_with_a_reason_and_other_files_still_sync() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.rs"), "fn alpha_one() -> u8 { 1 }\n").unwrap();
    let locked = dir.path().join("locked.txt");
    fs::write(&locked, "alpha beta gamma\n").unwrap();
    let Some(_guard) = lock(&locked) else { return };
    let source = register(&store, dir.path());

    let result = sync_source(&store, &source, None).expect("an unreadable file must not abort");

    assert_eq!(result.files_added, 1, "the readable file must be indexed");
    assert_eq!(indexed_paths(&store), vec!["alpha.rs".to_string()]);
    assert_eq!(result.skipped_files.len(), 1);
    assert_eq!(result.skipped_files[0].rel_path, "locked.txt");
    assert!(
        result.skipped_files[0].reason.starts_with("unreadable: "),
        "reason was: {}",
        result.skipped_files[0].reason
    );
}

#[test]
fn file_that_becomes_unreadable_loses_its_rows_and_is_indexed_again_once_readable() {
    let store = make_store();
    let dir = tempdir().unwrap();
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    let source = register(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    assert_eq!(indexed_paths(&store), vec!["notes.txt".to_string()]);

    let Some(guard) = lock(&notes) else { return };
    let skipped = sync_source(&store, &source, None).unwrap();
    assert!(indexed_paths(&store).is_empty(), "stale rows survived");
    assert_eq!(skipped.files_removed, 1);
    assert_eq!(skipped.skipped_files[0].rel_path, "notes.txt");

    drop(guard);
    let recovered = sync_source(&store, &source, None).unwrap();
    assert!(recovered.skipped_files.is_empty());
    assert_eq!(indexed_paths(&store), vec!["notes.txt".to_string()]);
}

#[test]
fn event_driven_sync_skips_an_unreadable_file_and_reports_it() {
    // Watch mode reindexes only the paths that events name, so it needs the same behavior.
    let store = make_store();
    let dir = tempdir().unwrap();
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    fs::write(dir.path().join("other.txt"), "delta epsilon\n").unwrap();
    let source = register(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    let Some(guard) = lock(&notes) else { return };
    let skipped = sync_paths(&store, &source, None, &["notes.txt".to_string()]).unwrap();
    assert_eq!(skipped.skipped_files.len(), 1);
    assert_eq!(skipped.skipped_files[0].rel_path, "notes.txt");
    assert!(skipped.skipped_files[0].reason.starts_with("unreadable: "));
    assert_eq!(indexed_paths(&store), vec!["other.txt".to_string()]);

    drop(guard);
    let recovered = sync_paths(&store, &source, None, &["notes.txt".to_string()]).unwrap();
    assert!(recovered.skipped_files.is_empty());
    assert_eq!(
        indexed_paths(&store),
        vec!["notes.txt".to_string(), "other.txt".to_string()]
    );
}

#[test]
fn event_driven_sync_does_not_report_unreadable_files_it_was_not_asked_about() {
    let store = make_store();
    let dir = tempdir().unwrap();
    let locked = dir.path().join("locked.txt");
    fs::write(&locked, "alpha beta gamma\n").unwrap();
    fs::write(dir.path().join("other.txt"), "delta epsilon\n").unwrap();
    let source = register(&store, dir.path());
    let Some(_guard) = lock(&locked) else { return };

    let result = sync_paths(&store, &source, None, &["other.txt".to_string()]).unwrap();

    assert!(
        result.skipped_files.is_empty(),
        "{:?}",
        result.skipped_files
    );
    assert_eq!(indexed_paths(&store), vec!["other.txt".to_string()]);
}

#[test]
fn unreadable_directory_still_fails_the_sync() {
    // The walk cannot say what the directory holds, so skipping it would silently drop files.
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("alpha.rs"), "fn alpha_one() -> u8 { 1 }\n").unwrap();
    let locked = dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("inner.txt"), "alpha beta gamma\n").unwrap();
    let Some(_guard) = lock(&locked) else { return };
    let source = register(&store, dir.path());

    let result = sync_source(&store, &source, None);

    assert!(result.is_err(), "a walk failure must surface");
    assert!(indexed_paths(&store).is_empty(), "nothing is written");
}
