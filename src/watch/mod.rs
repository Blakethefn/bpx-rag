//! Watch mode: keep registered sources synchronized with the filesystem.
//!
//! One watcher runs per database, enforced by an exclusive lease. A watcher registers a `notify`
//! watch for each source that has watch mode on, reconciles the source with a full sync before it
//! trusts events, and then reindexes only the paths that events name. Every write goes through the
//! writer lock that manual sync also takes. Status is written atomically beside the database.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::embed::Embed;
use crate::knowledge::lock::FileLock;
use crate::knowledge::sync::SkippedFile;

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

mod engine;

use engine::Engine;

#[cfg(test)]
pub(crate) use engine::{RegisterProbe, REGISTER_PROBE};

#[cfg(test)]
mod tests;
