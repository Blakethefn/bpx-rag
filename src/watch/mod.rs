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

const CONFIG_POLL: Duration = Duration::from_secs(1);
const EVENT_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub debounce: Duration,
    pub max_batch: Duration,
    pub retry_base: Duration,
    pub retry_max: Duration,
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
    Stopped,
    AlreadyRunning { pid: Option<u32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Reconciling,
    Watching,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub label: String,
    pub state: SourceState,
    pub reconciled_at: Option<i64>,
    pub batches: u64,
    pub reindexed_files: u64,
    pub last_error: Option<String>,
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

pub type EmbedderLoader = Box<dyn Fn() -> Option<Arc<dyn Embed>> + Send>;

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
