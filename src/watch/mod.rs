//! Watch mode: keep registered sources synchronized with the filesystem.
//!
//! One watcher runs per database, enforced by an exclusive lease. A watcher registers a `notify`
//! watch for each source that has watch mode on, reconciles the source with a full sync before it
//! trusts events, and then reindexes only the paths that events name. Every write goes through the
//! writer lock that manual sync also takes. Status is written atomically beside the database.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::embed::Embed;
use crate::knowledge::enrichment::build_enrichment_fn;
use crate::knowledge::lock::{writer_lock_path, FileLock};
use crate::knowledge::sync::{sync_paths, sync_source, EnrichmentFn, SkippedFile, SyncResult};
use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

/// How often the watcher rereads which sources have watch mode on.
const CONFIG_POLL: Duration = Duration::from_secs(1);
/// How long the loop waits for an event before doing other work.
const EVENT_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub struct WatchOptions {
    /// Quiet period that closes an event batch.
    pub debounce: Duration,
    /// Longest a batch may stay open while events keep arriving.
    pub max_batch: Duration,
    /// First retry delay after a failure. It doubles up to `retry_max`.
    pub retry_base: Duration,
    pub retry_max: Duration,
    /// How long a starting watcher keeps trying for the lease before it reports another watcher.
    /// `watch status` and `serve` probe the lease by taking it for an instant, and a watcher that
    /// starts then must not mistake that probe for a running watcher.
    pub lease_wait: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            max_batch: Duration::from_secs(5),
            retry_base: Duration::from_secs(1),
            retry_max: Duration::from_secs(60),
            lease_wait: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The stop flag was set and the lease was released.
    Stopped,
    /// Another process holds the watcher lease for this database.
    AlreadyRunning { pid: Option<u32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    /// The source is being reconciled with a full sync.
    Reconciling,
    /// The source is registered and reconciled, and events are reindexed.
    Watching,
    /// The last attempt failed. Retried with backoff.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub label: String,
    pub state: SourceState,
    /// Unix seconds of the last successful reconcile.
    pub reconciled_at: Option<i64>,
    /// Event batches processed since the watcher started.
    pub batches: u64,
    /// Files reindexed from events and retries, excluding the startup reconcile.
    pub reindexed_files: u64,
    pub last_error: Option<String>,
    /// Files the last sync could not index (not valid UTF-8, or unreadable), with the reason.
    #[serde(default)]
    pub skipped_files: Vec<SkippedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchStatus {
    pub pid: u32,
    pub sources: Vec<SourceStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Running { pid: Option<u32> },
    NotRunning,
}

/// Loads the embedder when the first source starts watching. Returns `None` when vectors are
/// unavailable, and changed chunks are then indexed without them, as `sync` does.
pub type EmbedderLoader = Box<dyn Fn() -> Option<Arc<dyn Embed>> + Send>;

/// Run the watcher until `stop` is set. Returns `AlreadyRunning` without touching the database
/// when another process holds the lease.
pub fn run_with_loader(
    db_path: &Path,
    options: &WatchOptions,
    stop: &AtomicBool,
    embedder_loader: EmbedderLoader,
) -> Result<RunOutcome> {
    let Some(lease) = acquire_lease(&lease_path(db_path), options.lease_wait)? else {
        let pid = read_status(db_path)?.map(|status| status.pid);
        return Ok(RunOutcome::AlreadyRunning { pid });
    };
    let mut engine = Engine::new(db_path, options.clone(), embedder_loader)?;
    engine.run_until(stop)?;
    drop(lease);
    Ok(RunOutcome::Stopped)
}

/// Take the watcher lease, retrying briefly. A holder that keeps it past `wait` is a running
/// watcher. One that releases it sooner was a probe (see `holder`).
fn acquire_lease(path: &Path, wait: Duration) -> Result<Option<FileLock>> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(lease) = FileLock::try_acquire(path)? {
            return Ok(Some(lease));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The last status the running watcher wrote, if any.
pub fn read_status(db_path: &Path) -> Result<Option<WatchStatus>> {
    let path = status_path(db_path);
    match fs::read(&path) {
        Ok(bytes) => {
            let status = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            Ok(Some(status))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Whether a watcher currently holds the lease for this database. The kernel releases the lease
/// when its holder exits, so a killed watcher is reported as not running.
pub fn holder(db_path: &Path) -> Result<Holder> {
    match FileLock::try_acquire(&lease_path(db_path))? {
        Some(lock) => {
            drop(lock);
            Ok(Holder::NotRunning)
        }
        None => {
            let pid = read_status(db_path)?.map(|status| status.pid);
            Ok(Holder::Running { pid })
        }
    }
}

fn lease_path(db_path: &Path) -> PathBuf {
    sibling(db_path, ".watch.lock")
}

fn status_path(db_path: &Path) -> PathBuf {
    sibling(db_path, ".watch.status.json")
}

fn sibling(db_path: &Path, suffix: &str) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

type Event = (String, notify::Result<notify::Event>);

/// One source under watch in this process.
struct Watched {
    source: KnowledgeSourceInfo,
    root: PathBuf,
    /// `None` until the `notify` watch is registered. Registration failure is an error state.
    registration: Option<RecommendedWatcher>,
    state: SourceState,
    reconciled_at: Option<i64>,
    batches: u64,
    reindexed_files: u64,
    last_error: Option<String>,
    skipped: Vec<SkippedFile>,
    retry_at: Option<Instant>,
    backoff: Duration,
    pending: BTreeSet<String>,
    need_full: bool,
    first_event: Option<Instant>,
    last_event: Option<Instant>,
}

struct Engine {
    db_path: PathBuf,
    options: WatchOptions,
    store: KnowledgeStore,
    embedder_loader: Option<EmbedderLoader>,
    watched: BTreeMap<String, Watched>,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    pid: u32,
    last_config_poll: Option<Instant>,
}

impl Engine {
    fn new(db_path: &Path, options: WatchOptions, embedder_loader: EmbedderLoader) -> Result<Self> {
        let store = KnowledgeStore::open_at(db_path)?;
        let (events_tx, events_rx) = mpsc::channel();
        Ok(Self {
            db_path: db_path.to_path_buf(),
            options,
            store,
            embedder_loader: Some(embedder_loader),
            watched: BTreeMap::new(),
            events_tx,
            events_rx,
            pid: std::process::id(),
            last_config_poll: None,
        })
    }

    fn run_until(&mut self, stop: &AtomicBool) -> Result<()> {
        while !stop.load(Ordering::SeqCst) {
            if self
                .last_config_poll
                .is_none_or(|last| last.elapsed() >= CONFIG_POLL)
            {
                self.refresh_sources()?;
                self.last_config_poll = Some(Instant::now());
            }
            self.retry_due();
            self.drain_events();
            self.flush_due_batches();
        }
        self.watched.clear();
        self.write_status();
        Ok(())
    }

    /// Start watching sources newly switched on, and stop watching sources switched off.
    fn refresh_sources(&mut self) -> Result<()> {
        let wanted = self.store.watched_sources()?;
        let wanted_labels: BTreeSet<String> =
            wanted.iter().map(|source| source.label.clone()).collect();
        self.watched
            .retain(|label, _| wanted_labels.contains(label));
        for source in wanted {
            let label = source.label.clone();
            if let Some(entry) = self.watched.get_mut(&label) {
                entry.source = source;
            } else {
                let root = PathBuf::from(&source.path);
                self.watched.insert(
                    label.clone(),
                    Watched {
                        source,
                        root,
                        registration: None,
                        state: SourceState::Reconciling,
                        reconciled_at: None,
                        batches: 0,
                        reindexed_files: 0,
                        last_error: None,
                        skipped: Vec::new(),
                        retry_at: None,
                        backoff: self.options.retry_base,
                        pending: BTreeSet::new(),
                        need_full: false,
                        first_event: None,
                        last_event: None,
                    },
                );
                self.start_source(&label);
            }
        }
        self.write_status();
        Ok(())
    }

    /// Load the embedder once, before the first reconcile that may need it.
    fn ensure_embedder(&mut self) {
        if let Some(loader) = self.embedder_loader.take() {
            if let Some(embedder) = loader() {
                self.store.set_embedder(embedder);
            }
        }
    }

    /// Register the `notify` watch, then reconcile. Events that arrive during the reconcile are
    /// queued, so nothing that changes after the watch starts can be missed.
    fn start_source(&mut self, label: &str) {
        self.ensure_embedder();
        let Some(entry) = self.watched.get_mut(label) else {
            return;
        };
        if entry.registration.is_none() {
            match register(label, &entry.root, self.events_tx.clone()) {
                Ok(watcher) => entry.registration = Some(watcher),
                Err(error) => {
                    fail(
                        entry,
                        format!("watch registration failed: {error:#}"),
                        &self.options,
                    );
                    self.write_status();
                    return;
                }
            }
        }
        self.reconcile(label);
    }

    fn reconcile(&mut self, label: &str) {
        let Some(entry) = self.watched.get_mut(label) else {
            return;
        };
        entry.state = SourceState::Reconciling;
        let result = locked_sync(
            &self.db_path,
            &self.store,
            &entry.source,
            |store, source, enrichment| sync_source(store, source, enrichment),
        );
        match result {
            Ok(result) => {
                entry.skipped = result.skipped_files;
                entry.state = SourceState::Watching;
                entry.reconciled_at = Some(unix_now());
                entry.last_error = None;
                entry.backoff = self.options.retry_base;
                entry.retry_at = None;
                entry.need_full = false;
                entry.pending.clear();
                entry.first_event = None;
                entry.last_event = None;
            }
            Err(error) => fail(entry, format!("{error:#}"), &self.options),
        }
        self.write_status();
    }

    fn retry_due(&mut self) {
        let due: Vec<String> = self
            .watched
            .iter()
            .filter(|(_, entry)| {
                entry.state == SourceState::Error
                    && entry.retry_at.is_some_and(|at| Instant::now() >= at)
            })
            .map(|(label, _)| label.clone())
            .collect();
        for label in due {
            let registered = self
                .watched
                .get(&label)
                .is_some_and(|entry| entry.registration.is_some());
            if registered {
                self.reconcile(&label);
            } else {
                self.start_source(&label);
            }
        }
    }

    /// Move queued events into each source's pending set. A notify error or an overflow asks for a
    /// full reconcile instead, because the lost events cannot be recovered.
    fn drain_events(&mut self) {
        let first = self.events_rx.recv_timeout(EVENT_POLL).ok();
        let rest = std::iter::from_fn(|| self.events_rx.try_recv().ok());
        for (label, outcome) in first.into_iter().chain(rest) {
            let Some(entry) = self.watched.get_mut(&label) else {
                continue;
            };
            let now = Instant::now();
            match outcome {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Access(_)) {
                        // Reads (our own included) are not changes.
                        continue;
                    }
                    if event.need_rescan() {
                        entry.need_full = true;
                    }
                    for path in &event.paths {
                        match path.strip_prefix(&entry.root) {
                            Ok(relative) if !relative.as_os_str().is_empty() => {
                                entry
                                    .pending
                                    .insert(relative.to_string_lossy().replace('\\', "/"));
                            }
                            _ => entry.need_full = true,
                        }
                    }
                }
                Err(error) => {
                    entry.need_full = true;
                    entry.last_error = Some(format!("watch event error: {error}"));
                }
            }
            entry.first_event.get_or_insert(now);
            entry.last_event = Some(now);
        }
    }

    fn flush_due_batches(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self
            .watched
            .iter()
            .filter(|(_, entry)| {
                entry.state == SourceState::Watching
                    && (entry.need_full || !entry.pending.is_empty())
                    && entry.last_event.is_some_and(|last| {
                        now.duration_since(last) >= self.options.debounce
                            || entry.first_event.is_some_and(|first| {
                                now.duration_since(first) >= self.options.max_batch
                            })
                    })
            })
            .map(|(label, _)| label.clone())
            .collect();
        for label in due {
            self.process_batch(&label);
        }
    }

    fn process_batch(&mut self, label: &str) {
        let Some(entry) = self.watched.get_mut(label) else {
            return;
        };
        let full = entry.need_full
            || needs_full_scan(&self.store, &entry.source, &entry.root, &entry.pending);
        let paths: Vec<String> = entry.pending.iter().cloned().collect();
        entry.pending.clear();
        entry.need_full = false;
        entry.first_event = None;
        entry.last_event = None;

        let result = locked_sync(
            &self.db_path,
            &self.store,
            &entry.source,
            |store, source, enrichment| {
                if full {
                    sync_source(store, source, enrichment)
                } else {
                    sync_paths(store, source, enrichment, &paths)
                }
            },
        );
        match result {
            Ok(outcome) => {
                if full {
                    entry.skipped = outcome.skipped_files.clone();
                } else {
                    entry.skipped.retain(|file| !paths.contains(&file.rel_path));
                    entry.skipped.extend(outcome.skipped_files.iter().cloned());
                }
                entry.batches += 1;
                entry.reindexed_files += changed_files(&outcome);
                entry.state = SourceState::Watching;
                entry.last_error = None;
                entry.backoff = self.options.retry_base;
            }
            Err(error) => fail(entry, format!("{error:#}"), &self.options),
        }
        self.write_status();
    }

    fn write_status(&self) {
        let status = WatchStatus {
            pid: self.pid,
            sources: self
                .watched
                .iter()
                .map(|(label, entry)| SourceStatus {
                    label: label.clone(),
                    state: entry.state,
                    reconciled_at: entry.reconciled_at,
                    batches: entry.batches,
                    reindexed_files: entry.reindexed_files,
                    last_error: entry.last_error.clone(),
                    skipped_files: entry.skipped.clone(),
                })
                .collect(),
        };
        // A status write that fails must not stop watching. The next status write retries it.
        let _ = write_atomically(&status_path(&self.db_path), &status);
    }
}

fn register(label: &str, root: &Path, events: Sender<Event>) -> Result<RecommendedWatcher> {
    register_failpoint()?;
    let label = label.to_string();
    let mut watcher = notify::recommended_watcher(move |outcome| {
        let _ = events.send((label.clone(), outcome));
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

// Test hook: fail the next registrations without touching host inotify resources. A test
// installs a probe on the thread that runs the engine. Like `REBUILD_FAILPOINT` in
// `knowledge/identifiers.rs`, the hook is thread-local, so parallel tests do not interfere.
#[cfg(test)]
pub(crate) struct RegisterProbe {
    /// How many upcoming registrations fail.
    pub(crate) fail_first: std::sync::atomic::AtomicU32,
    /// When each registration was attempted.
    pub(crate) attempts: std::sync::Mutex<Vec<Instant>>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static REGISTER_PROBE: std::cell::RefCell<Option<Arc<RegisterProbe>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn register_failpoint() -> Result<()> {
    let Some(probe) = REGISTER_PROBE.with(|probe| probe.borrow().clone()) else {
        return Ok(());
    };
    probe.attempts.lock().unwrap().push(Instant::now());
    let failing = probe
        .fail_first
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok();
    if failing {
        anyhow::bail!("injected registration failure: Too many open files (os error 24)");
    }
    Ok(())
}

#[cfg(not(test))]
fn register_failpoint() -> Result<()> {
    Ok(())
}

/// Record a failure and schedule the next attempt with doubling backoff, capped at `retry_max`.
fn fail(entry: &mut Watched, message: String, options: &WatchOptions) {
    entry.state = SourceState::Error;
    entry.last_error = Some(message);
    entry.need_full = true;
    entry.retry_at = Some(Instant::now() + entry.backoff);
    entry.backoff = (entry.backoff * 2).min(options.retry_max);
}

/// Run `operation` while holding the writer lock. Manual sync takes the same lock.
fn locked_sync(
    db_path: &Path,
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    operation: impl FnOnce(
        &KnowledgeStore,
        &KnowledgeSourceInfo,
        EnrichmentFn<'_>,
    ) -> Result<SyncResult>,
) -> Result<SyncResult> {
    let _writer = FileLock::acquire(&writer_lock_path(db_path))?;
    let enrichment = build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrichment_ref = enrichment
        .as_ref()
        .map(|function| function.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    operation(store, source, enrichment_ref)
}

/// Directory-level events, and paths whose indexed children disappeared, need a full reconcile.
fn needs_full_scan(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    root: &Path,
    paths: &BTreeSet<String>,
) -> bool {
    paths.iter().any(|relative| {
        if root.join(relative).is_dir() {
            return true;
        }
        let prefix = format!("{}/", relative.replace('\\', "/"));
        let pattern = format!("{}%", escape_like(&prefix));
        store
            .conn()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_files
                 WHERE source_id = ?1 AND rel_path LIKE ?2 ESCAPE '\\')",
                rusqlite::params![source.id, pattern],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn changed_files(result: &SyncResult) -> u64 {
    (result.files_added + result.files_updated + result.files_removed) as u64
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Write to a temporary file, then rename it over the target, so readers never see a partial file.
fn write_atomically(path: &Path, status: &WatchStatus) -> Result<()> {
    let temporary = sibling(path, ".tmp");
    let bytes = serde_json::to_vec(status)?;
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
