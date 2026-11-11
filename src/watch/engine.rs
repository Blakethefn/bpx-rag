use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::RecommendedWatcher;

use crate::knowledge::sync::SkippedFile;
use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

use super::{
    sibling, status_path, EmbedderLoader, SourceState, SourceStatus, WatchOptions, WatchStatus,
    CONFIG_POLL,
};

mod events;
mod sources;

#[cfg(test)]
pub(crate) use events::{REGISTER_PROBE, RegisterProbe};

type Event = (String, notify::Result<notify::Event>);

/// One source under watch in this process.
pub(in crate::watch) struct Watched {
    source: KnowledgeSourceInfo,
    root: PathBuf,
    /// `None` until the `notify` watch is registered. Registration failure is an error state.
    pub(in crate::watch) registration: Option<RecommendedWatcher>,
    pub(in crate::watch) state: SourceState,
    pub(in crate::watch) reconciled_at: Option<i64>,
    batches: u64,
    reindexed_files: u64,
    pub(in crate::watch) last_error: Option<String>,
    skipped: Vec<SkippedFile>,
    pub(in crate::watch) retry_at: Option<Instant>,
    pub(in crate::watch) backoff: Duration,
    pending: BTreeSet<String>,
    need_full: bool,
    first_event: Option<Instant>,
    last_event: Option<Instant>,
}

pub(in crate::watch) struct Engine {
    db_path: PathBuf,
    options: WatchOptions,
    store: KnowledgeStore,
    embedder_loader: Option<EmbedderLoader>,
    pub(in crate::watch) watched: BTreeMap<String, Watched>,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    pid: u32,
    last_config_poll: Option<Instant>,
}

impl Engine {
    pub(in crate::watch) fn new(
        db_path: &Path,
        options: WatchOptions,
        embedder_loader: EmbedderLoader,
    ) -> Result<Self> {
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

    pub(in crate::watch) fn run_until(&mut self, stop: &AtomicBool) -> Result<()> {
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

    pub(in crate::watch) fn write_status(&self) {
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

/// Record a failure and schedule the next attempt with doubling backoff, capped at `retry_max`.
fn fail(entry: &mut Watched, message: String, options: &WatchOptions) {
    entry.state = SourceState::Error;
    entry.last_error = Some(message);
    entry.need_full = true;
    entry.retry_at = Some(Instant::now() + entry.backoff);
    entry.backoff = (entry.backoff * 2).min(options.retry_max);
}

/// Write to a temporary file, then rename it over the target, so readers never see a partial file.
fn write_atomically(path: &Path, status: &WatchStatus) -> Result<()> {
    let temporary = sibling(path, ".tmp");
    let bytes = serde_json::to_vec(status)?;
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}
