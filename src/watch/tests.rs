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

mod registration;
mod sync;

fn fast_options() -> WatchOptions {
    WatchOptions {
        debounce: Duration::from_millis(300),
        max_batch: Duration::from_secs(3),
        retry_base: Duration::from_millis(100),
        retry_max: Duration::from_millis(500),
        lease_wait: Duration::from_millis(150),
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
