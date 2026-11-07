use super::*;
use crate::knowledge::lock::{writer_lock_path, FileLock};
use crate::knowledge::sync::sync_source;
use crate::knowledge::KnowledgeStore;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;
use tempfile::{tempdir, TempDir};

fn fast_options() -> WatchOptions {
    WatchOptions {
        debounce: Duration::from_millis(300),
        max_batch: Duration::from_secs(3),
        retry_base: Duration::from_millis(100),
        retry_max: Duration::from_millis(500),
    }
}

fn run_without_vectors(db: &Path, options: &WatchOptions, stop: &AtomicBool) -> Result<RunOutcome> {
    run_with_loader(db, options, stop, Box::new(|| None))
}

fn lease_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(".watch.lock");
    PathBuf::from(name)
}

struct Fixture {
    _root: TempDir,
    db: PathBuf,
    source_dir: PathBuf,
}

/// A database file with one source over a fresh directory, synced, and watch enabled.
fn fixture() -> Fixture {
    let root = tempdir().unwrap();
    let db = root.path().join("knowledge.db");
    let source_dir = root.path().join("src");
    fs::create_dir(&source_dir).unwrap();
    fs::write(source_dir.join("a.rs"), "pub fn alpha() -> u8 { 1 }\n").unwrap();
    let store = KnowledgeStore::open_at(&db).unwrap();
    store
        .add_source("test", source_dir.to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    sync_source(&store, &source, None).unwrap();
    store.set_watch("test", true).unwrap();
    Fixture {
        _root: root,
        db,
        source_dir,
    }
}

fn start(db: &Path) -> (JoinHandle<Result<RunOutcome>>, Arc<AtomicBool>) {
    let stop = Arc::new(AtomicBool::new(false));
    let db = db.to_path_buf();
    let thread_stop = stop.clone();
    let handle =
        std::thread::spawn(move || run_without_vectors(&db, &fast_options(), &thread_stop));
    (handle, stop)
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    condition()
}

fn source_status(db: &Path, label: &str) -> Option<SourceStatus> {
    read_status(db)
        .unwrap()?
        .sources
        .into_iter()
        .find(|source| source.label == label)
}

fn watching(db: &Path) -> bool {
    matches!(
        source_status(db, "test"),
        Some(status) if status.state == SourceState::Watching && status.reconciled_at.is_some()
    )
}

fn symbols(db: &Path) -> Vec<String> {
    let store = KnowledgeStore::open_at(db).unwrap();
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

fn indexed_files(db: &Path) -> Vec<String> {
    let store = KnowledgeStore::open_at(db).unwrap();
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

fn stop_and_join(stop: Arc<AtomicBool>, handle: JoinHandle<Result<RunOutcome>>) -> RunOutcome {
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap().unwrap()
}

#[test]
fn run_reports_already_running_while_another_watcher_holds_the_lease() {
    let fixture = fixture();
    let _held = FileLock::acquire(&lease_path(&fixture.db)).unwrap();
    let stop = AtomicBool::new(true);
    let outcome = run_without_vectors(&fixture.db, &fast_options(), &stop).unwrap();
    assert!(matches!(outcome, RunOutcome::AlreadyRunning { .. }));
}

#[test]
fn stopping_the_watcher_releases_the_lease() {
    let fixture = fixture();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
    assert!(matches!(holder(&fixture.db).unwrap(), Holder::NotRunning));
}

#[test]
fn startup_reconcile_applies_add_modify_rename_and_delete_made_while_stopped() {
    let fixture = fixture();
    fs::write(
        fixture.source_dir.join("b.rs"),
        "pub fn beta() -> u8 { 2 }\n",
    )
    .unwrap();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);

    // Changes made while no watcher runs.
    fs::write(
        fixture.source_dir.join("a.rs"),
        "pub fn alpha_v2() -> u8 { 1 }\n",
    )
    .unwrap();
    fs::rename(
        fixture.source_dir.join("b.rs"),
        fixture.source_dir.join("c.rs"),
    )
    .unwrap();
    fs::write(
        fixture.source_dir.join("d.rs"),
        "pub fn delta() -> u8 { 4 }\n",
    )
    .unwrap();
    fs::remove_file(fixture.source_dir.join("d.rs")).unwrap();

    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || {
        symbols(&fixture.db) == vec!["alpha_v2", "beta"]
            && indexed_files(&fixture.db) == vec!["a.rs", "c.rs"]
    }));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}

#[test]
fn rapid_consecutive_changes_produce_one_reindex() {
    let fixture = fixture();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));
    let baseline = source_status(&fixture.db, "test").unwrap().reindexed_files;

    let path = fixture.source_dir.join("a.rs");
    for version in 0..30 {
        fs::write(
            &path,
            format!("pub fn burst_{version:02}() -> u8 {{ {version} }}\n"),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["burst_29"]));
    std::thread::sleep(Duration::from_millis(800));
    let after = source_status(&fixture.db, "test").unwrap();
    assert_eq!(
        after.reindexed_files - baseline,
        1,
        "a burst of edits to one file is one reindex"
    );
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}

#[test]
fn watched_edit_updates_symbols_and_watched_delete_removes_rows() {
    let fixture = fixture();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));

    fs::write(
        fixture.source_dir.join("a.rs"),
        "pub fn renamed() -> u8 { 1 }\n",
    )
    .unwrap();
    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["renamed"]));

    fs::remove_file(fixture.source_dir.join("a.rs")).unwrap();
    assert!(wait_for(Duration::from_secs(10), || indexed_files(
        &fixture.db
    )
    .is_empty()));
    assert!(symbols(&fixture.db).is_empty());
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}

#[test]
fn missing_source_is_reported_as_an_error_and_the_watcher_keeps_running() {
    let fixture = fixture();
    fs::remove_dir_all(&fixture.source_dir).unwrap();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || {
        matches!(
            source_status(&fixture.db, "test"),
            Some(status) if status.state == SourceState::Error && status.last_error.is_some()
        )
    }));
    assert!(
        !handle.is_finished(),
        "a failing source must not end the watcher"
    );
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}

#[test]
fn concurrent_manual_and_watched_sync_keep_the_database_consistent() {
    let fixture = fixture();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));

    for round in 0..10 {
        fs::write(
            fixture.source_dir.join("a.rs"),
            format!("pub fn round_{round:02}() -> u8 {{ {round} }}\n"),
        )
        .unwrap();
        // A manual sync takes the same writer lock the watcher takes.
        let _writer = FileLock::acquire(&writer_lock_path(&fixture.db)).unwrap();
        let store = KnowledgeStore::open_at(&fixture.db).unwrap();
        let source = store.get_source("test").unwrap().unwrap();
        sync_source(&store, &source, None).unwrap();
    }

    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["round_09"]));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);

    let store = KnowledgeStore::open_at(&fixture.db).unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    let settled = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        (
            settled.files_added,
            settled.files_updated,
            settled.files_removed
        ),
        (0, 0, 0),
        "after the watcher stops, the index must already match the disk"
    );
    let integrity: String = store
        .conn()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

// ── registration failure ────────────────────────────────────────────────────────────────────
//
// Registration can fail for real when the per-user inotify limits are used up. These tests
// inject the failure instead, so they never consume host resources.

fn install_probe(fail_first: u32) -> Arc<RegisterProbe> {
    let probe = Arc::new(RegisterProbe {
        fail_first: std::sync::atomic::AtomicU32::new(fail_first),
        attempts: std::sync::Mutex::new(Vec::new()),
    });
    REGISTER_PROBE.with(|slot| *slot.borrow_mut() = Some(probe.clone()));
    probe
}

fn attempt_count(probe: &RegisterProbe) -> usize {
    probe.attempts.lock().unwrap().len()
}

fn sleep_until(at: Instant) {
    let now = Instant::now();
    if at > now {
        std::thread::sleep(at - now);
    }
}

#[test]
fn registration_failure_sets_error_retries_with_backoff_and_recovers() {
    let fixture = fixture();
    let probe = install_probe(2);
    let mut engine = Engine::new(&fixture.db, fast_options(), Box::new(|| None)).unwrap();

    // First attempt fails: error state, the reason is reported, and no reconcile ran.
    engine.refresh_sources().unwrap();
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Error);
    assert!(entry.registration.is_none());
    assert!(
        entry.reconciled_at.is_none(),
        "no reconcile without a watch"
    );
    let message = entry.last_error.clone().unwrap();
    assert!(
        message.starts_with("watch registration failed: "),
        "got: {message}"
    );
    assert!(message.contains("Too many open files"), "got: {message}");
    let first_retry = entry.retry_at.expect("a retry must be scheduled");
    assert_eq!(entry.backoff, Duration::from_millis(200), "backoff doubles");
    let reported = source_status(&fixture.db, "test").unwrap();
    assert_eq!(reported.state, SourceState::Error);
    assert_eq!(reported.last_error.as_deref(), Some(message.as_str()));

    // Before the retry is due nothing is attempted.
    engine.retry_due();
    assert_eq!(attempt_count(&probe), 1);

    // Second attempt fails too, and the delay doubles again.
    sleep_until(first_retry);
    engine.retry_due();
    assert_eq!(attempt_count(&probe), 2);
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Error);
    assert_eq!(entry.backoff, Duration::from_millis(400));
    let second_retry = entry.retry_at.unwrap();

    // Third attempt succeeds: registered, reconciled, and the error and backoff are cleared.
    sleep_until(second_retry);
    engine.retry_due();
    assert_eq!(attempt_count(&probe), 3);
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Watching);
    assert!(entry.registration.is_some());
    assert!(entry.reconciled_at.is_some());
    assert!(entry.last_error.is_none());
    assert!(entry.retry_at.is_none());
    assert_eq!(entry.backoff, fast_options().retry_base);
    assert!(watching(&fixture.db));

    // The delays between attempts follow the backoff (lower bounds only, so slow hosts pass).
    let attempts = probe.attempts.lock().unwrap().clone();
    assert!(attempts[1] - attempts[0] >= Duration::from_millis(100));
    assert!(attempts[2] - attempts[1] >= Duration::from_millis(200));
}

#[test]
fn registration_backoff_is_capped_at_the_maximum() {
    let fixture = fixture();
    let probe = install_probe(u32::MAX);
    let mut engine = Engine::new(&fixture.db, fast_options(), Box::new(|| None)).unwrap();
    engine.refresh_sources().unwrap();

    for _ in 0..4 {
        let retry_at = engine.watched["test"].retry_at.unwrap();
        sleep_until(retry_at);
        engine.retry_due();
    }

    assert_eq!(attempt_count(&probe), 5);
    assert_eq!(engine.watched["test"].state, SourceState::Error);
    assert_eq!(
        engine.watched["test"].backoff,
        fast_options().retry_max,
        "the delay must not grow past retry_max"
    );
}

#[test]
fn watcher_thread_recovers_from_registration_failures_and_then_follows_edits() {
    let fixture = fixture();
    let stop = Arc::new(AtomicBool::new(false));
    let probe = Arc::new(RegisterProbe {
        fail_first: std::sync::atomic::AtomicU32::new(2),
        attempts: std::sync::Mutex::new(Vec::new()),
    });
    let handle = {
        let db = fixture.db.clone();
        let stop = stop.clone();
        let probe = probe.clone();
        std::thread::spawn(move || {
            REGISTER_PROBE.with(|slot| *slot.borrow_mut() = Some(probe));
            run_without_vectors(&db, &fast_options(), &stop)
        })
    };

    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));
    assert_eq!(attempt_count(&probe), 3);
    assert!(
        !handle.is_finished(),
        "registration failures must not end the watcher"
    );

    fs::write(
        fixture.source_dir.join("a.rs"),
        "pub fn after_recovery() -> u8 { 1 }\n",
    )
    .unwrap();
    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["after_recovery"]));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}
