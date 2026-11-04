use super::*;
use crate::knowledge::query::{knowledge_search_report, SearchWeights};
use crate::knowledge::sync::sync_source;
use crate::knowledge::{open_in_memory, KnowledgeSourceInfo};
use rusqlite::params;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

const HOUR: Duration = Duration::from_secs(3600);

fn ns(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

fn now_ns() -> i64 {
    ns(SystemTime::now())
}

fn set_mtime(path: &Path, time: SystemTime) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

/// Register a source over `dir` and sync it. Files written before this call are aged an hour so
/// the metadata check is not racy unless a test makes it so.
fn sync_dir(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source("test", dir.to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
    source
}

fn write_aged(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    set_mtime(&path, SystemTime::now() - HOUR);
    path
}

fn record(store: &KnowledgeStore, rel_path: &str) -> IndexedFile {
    store
        .conn()
        .query_row(
            "SELECT rel_path, content_hash, size_bytes, mtime_ns, stat_checked_ns
             FROM knowledge_files WHERE rel_path = ?1",
            params![rel_path],
            |row| {
                Ok(IndexedFile {
                    rel_path: row.get(0)?,
                    content_hash: row.get(1)?,
                    size_bytes: row.get(2)?,
                    mtime_ns: row.get(3)?,
                    stat_checked_ns: row.get(4)?,
                })
            },
        )
        .unwrap()
}

fn check(store: &KnowledgeStore, dir: &Path, rel_path: &str) -> Freshness {
    check_indexed_file(dir, &record(store, rel_path), now_ns())
}

fn freshness(state: FreshnessState, basis: FreshnessBasis) -> Freshness {
    Freshness { state, basis }
}

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

/// Benchmark: time the freshness check for the results of real searches. Run with
/// `cargo test --release freshness_check_latency -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark"]
fn freshness_check_latency_at_10_50_and_200_results() {
    let store = open_in_memory().unwrap();
    let dir = tempdir().unwrap();
    for index in 0..400 {
        write_aged(
            dir.path(),
            &format!("file{index:03}.md"),
            &format!(
                "# File {index}\nshared phrase for result {index}, with enough text to chunk.\n"
            ),
        );
    }
    sync_dir(&store, dir.path());
    for limit in [10u32, 50, 200] {
        let report = knowledge_search_report(
            &store,
            "shared phrase",
            None,
            None,
            limit,
            &SearchWeights::default(),
            200,
        )
        .unwrap();
        let mut samples = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            let states = check_results(&store, &report.results).unwrap();
            samples.push(start.elapsed());
            assert_eq!(states.len(), report.results.len());
        }
        samples.sort();
        println!(
            "freshness_check results={} median_us={} p95_us={}",
            report.results.len(),
            samples[samples.len() / 2].as_micros(),
            samples[samples.len() * 95 / 100].as_micros()
        );
    }
}
