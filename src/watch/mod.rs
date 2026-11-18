use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::embed::Embed;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Stopped,
    AlreadyRunning { pid: Option<u32> },
    Disabled { warning: String },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Running { pid: Option<u32> },
    NotRunning,
    Disabled { warning: String },
}

pub type EmbedderLoader = Box<dyn Fn() -> Option<Arc<dyn Embed>> + Send>;

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

pub(super) fn status_path(db_path: &Path) -> std::path::PathBuf {
    sibling(db_path, ".watch.status.json")
}

pub(super) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

mod engine;
mod lease;
mod runtime;

pub use lease::{holder, run_with_loader};

pub(crate) fn permission_warning(error: &anyhow::Error) -> Option<String> {
    runtime::permission_denied(error).then(|| {
        format!(
            "background file watching is disabled in read-only or sandboxed mode: {error:#}"
        )
    })
}

#[cfg(test)]
pub(crate) use engine::{RegisterProbe, REGISTER_PERMISSION_FAIL, REGISTER_PROBE};

#[cfg(test)]
pub(crate) use lease::LEASE_PERMISSION_FAIL;

#[cfg(test)]
mod tests;
