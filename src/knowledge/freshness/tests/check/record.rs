use super::super::super::*;
use super::super::{check, freshness, sync_dir, write_aged};
use crate::knowledge::open_in_memory;
use crate::knowledge::sync::sync_source;
use std::fs;
use tempfile::tempdir;

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
