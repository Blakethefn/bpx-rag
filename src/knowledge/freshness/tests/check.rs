use super::super::*;
use super::{check, freshness, set_mtime, sync_dir, write_aged};
use crate::knowledge::open_in_memory;
use crate::knowledge::sync::sync_source;
use std::fs;
use std::time::{Duration, SystemTime};
use tempfile::tempdir;

#[test]
fn unchanged_file_is_fresh_by_metadata() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());

    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Fresh, FreshnessBasis::Metadata)
    );
}

#[test]
fn edited_file_with_a_new_size_is_stale_changed() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());

    write_aged(dir.path(), "a.md", "# A\nalpha content, now longer\n");
    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Stale, FreshnessBasis::Changed)
    );
}

#[test]
fn same_size_edit_with_a_new_mtime_is_stale_found_by_hash() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.rs", "fn one() -> u8 { 1 }\n");
    sync_dir(&store, dir.path());

    fs::write(&path, "fn two() -> u8 { 2 }\n").unwrap();
    assert_eq!(
        check(&store, dir.path(), "a.rs"),
        freshness(FreshnessState::Stale, FreshnessBasis::Changed)
    );
}

#[test]
fn same_size_edit_with_the_indexed_mtime_restored_is_the_documented_metadata_limit() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.rs", "fn one() -> u8 { 1 }\n");
    let indexed_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    sync_dir(&store, dir.path());

    fs::write(&path, "fn two() -> u8 { 2 }\n").unwrap();
    set_mtime(&path, indexed_mtime);
    // Size and mtime both match the index and the record is not racy, so metadata alone is trusted.
    // Sync and watch mode both detect this; see the Stage 7 limitations.
    assert_eq!(
        check(&store, dir.path(), "a.rs"),
        freshness(FreshnessState::Fresh, FreshnessBasis::Metadata)
    );
}

#[test]
fn racy_record_is_checked_by_hash_even_when_size_and_mtime_match() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.rs");
    // Written just before the sync: the mtime is within the granularity margin of the stat time.
    fs::write(&path, "fn one() -> u8 { 1 }\n").unwrap();
    let indexed_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    sync_dir(&store, dir.path());

    fs::write(&path, "fn two() -> u8 { 2 }\n").unwrap();
    set_mtime(&path, indexed_mtime);
    assert_eq!(
        check(&store, dir.path(), "a.rs"),
        freshness(FreshnessState::Stale, FreshnessBasis::Changed)
    );
}

#[test]
fn touched_file_with_unchanged_content_is_fresh_by_hash() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());

    set_mtime(&path, SystemTime::now());
    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Fresh, FreshnessBasis::Hash)
    );
}

#[test]
fn future_mtime_is_not_trusted_and_is_checked_by_hash() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    set_mtime(&path, SystemTime::now() + Duration::from_secs(86_400));
    sync_dir(&store, dir.path());

    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Fresh, FreshnessBasis::Hash)
    );
}

#[test]
fn deleted_file_is_stale_missing() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());

    fs::remove_file(&path).unwrap();
    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Stale, FreshnessBasis::Missing)
    );
}

#[test]
fn unreadable_changed_file_is_unknown_not_fresh() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());

    set_mtime(&path, SystemTime::now());
    fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();
    if fs::File::open(&path).is_ok() {
        // Running as root: permissions do not apply, so the scenario cannot be produced.
        eprintln!("skipping: permissions are not enforced for this user");
        return;
    }
    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Unknown, FreshnessBasis::Unreadable)
    );
}

#[test]
fn removed_source_directory_is_unknown_source_unavailable() {
    let store = open_in_memory().unwrap();
    let parent = tempdir().unwrap();
    let dir = parent.path().join("source");
    fs::create_dir(&dir).unwrap();
    write_aged(&dir, "a.md", "# A\nalpha content\n");
    sync_dir(&store, &dir);

    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        check(&store, &dir, "a.md"),
        freshness(FreshnessState::Unknown, FreshnessBasis::SourceUnavailable)
    );
}

#[test]
fn legacy_row_without_metadata_is_unknown_until_a_sync_records_it() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());
    store
        .conn()
        .execute(
            "UPDATE knowledge_files SET mtime_ns = NULL, stat_checked_ns = NULL",
            [],
        )
        .unwrap();

    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Unknown, FreshnessBasis::NotRecorded)
    );

    let source = store.get_source("test").unwrap().unwrap();
    sync_source(&store, &source, None).unwrap();
    assert_eq!(
        check(&store, dir.path(), "a.md"),
        freshness(FreshnessState::Fresh, FreshnessBasis::Metadata)
    );
}
