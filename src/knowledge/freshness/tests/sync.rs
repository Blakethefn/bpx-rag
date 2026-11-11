use super::super::*;
use super::{now_ns, ns, record, sync_dir, write_aged};
use crate::knowledge::open_in_memory;
use crate::knowledge::query::{knowledge_search_report, SearchWeights};
use crate::knowledge::sync::sync_source;
use std::fs;
use tempfile::tempdir;

#[test]
fn sync_records_the_metadata_it_read_before_the_content() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    let path = write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    let before = now_ns();
    sync_dir(&store, dir.path());

    let stored = record(&store, "a.md");
    assert_eq!(stored.size_bytes, fs::metadata(&path).unwrap().len() as i64);
    assert_eq!(
        stored.mtime_ns,
        Some(ns(fs::metadata(&path).unwrap().modified().unwrap()))
    );
    let checked = stored.stat_checked_ns.expect("stat time recorded");
    assert!(checked >= before, "the stat must be taken during this sync");
}

#[test]
fn check_results_marks_each_returned_result_by_its_own_file() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    write_aged(dir.path(), "a.md", "# Alpha\nshared phrase for the query\n");
    write_aged(dir.path(), "b.md", "# Beta\nshared phrase for the query\n");
    sync_dir(&store, dir.path());
    write_aged(
        dir.path(),
        "a.md",
        "# Alpha\nshared phrase for the query, edited\n",
    );

    let report = knowledge_search_report(
        &store,
        "shared phrase",
        None,
        None,
        10,
        &SearchWeights::default(),
        200,
    )
    .unwrap();
    let states = check_results(&store, &report.results).unwrap();
    assert_eq!(states.len(), report.results.len());
    for (result, state) in report.results.iter().zip(&states) {
        match result.file_path.as_str() {
            "a.md" => assert_eq!(state.state, FreshnessState::Stale),
            "b.md" => assert_eq!(state.state, FreshnessState::Fresh),
            other => panic!("unexpected result file {other}"),
        }
    }
}

#[test]
fn sync_backfills_missing_metadata_without_rechunking() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    write_aged(dir.path(), "a.md", "# A\nalpha content\n");
    sync_dir(&store, dir.path());
    let chunks_before: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    store
        .conn()
        .execute(
            "UPDATE knowledge_files SET mtime_ns = NULL, stat_checked_ns = NULL",
            [],
        )
        .unwrap();

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();
    assert_eq!(result.files_updated, 0);
    assert_eq!(result.chunks_indexed, 0);
    assert!(record(&store, "a.md").mtime_ns.is_some());
    let chunks_after: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(chunks_before, chunks_after);
}
