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
    sibling, EmbedderLoader, SourceState, SourceStatus, WatchOptions, WatchStatus, CONFIG_POLL,
};

mod events;
mod sources;

#[cfg(test)]
pub(crate) use events::{RegisterProbe, REGISTER_PERMISSION_FAIL, REGISTER_PROBE};

type Event = (String, notify::Result<notify::Event>);

pub(in crate::watch) struct Watched {
    source: KnowledgeSourceInfo,
    root: PathBuf,
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
    runtime_dir: PathBuf,
    options: WatchOptions,
    store: KnowledgeStore,
    embedder_loader: Option<EmbedderLoader>,
    pub(in crate::watch) watched: BTreeMap<String, Watched>,
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    pid: u32,
    last_config_poll: Option<Instant>,
    disabled_warning: Option<String>,
}

impl Engine {
    pub(in crate::watch) fn new(
        db_path: &Path,
        options: WatchOptions,
        embedder_loader: EmbedderLoader,
    ) -> Result<Self> {
        let store = KnowledgeStore::open_at(db_path)?;
        let runtime_dir = super::runtime::directory(db_path)?;
        let (events_tx, events_rx) = mpsc::channel();
        Ok(Self {
            db_path: db_path.to_path_buf(),
            runtime_dir,
            options,
            store,
            embedder_loader: Some(embedder_loader),
            watched: BTreeMap::new(),
            events_tx,
            events_rx,
            pid: std::process::id(),
            last_config_poll: None,
            disabled_warning: None,
        })
    }

    pub(in crate::watch) fn run_until(
        &mut self,
        stop: &AtomicBool,
    ) -> Result<super::RunOutcome> {
        while !stop.load(Ordering::SeqCst) {
            if self
                .last_config_poll
                .is_none_or(|last| last.elapsed() >= CONFIG_POLL)
            {
                self.refresh_sources()?;
                self.last_config_poll = Some(Instant::now());
            }
            if let Some(warning) = &self.disabled_warning {
                return Ok(super::RunOutcome::Disabled {
                    warning: warning.clone(),
                });
            }
            self.retry_due();
            self.drain_events();
            if let Some(warning) = &self.disabled_warning {
                return Ok(super::RunOutcome::Disabled {
                    warning: warning.clone(),
                });
            }
            self.flush_due_batches();
        }
        self.watched.clear();
        self.write_status();
        Ok(self.disabled_warning.clone().map_or(
            super::RunOutcome::Stopped,
            |warning| super::RunOutcome::Disabled { warning },
        ))
    }

    pub(in crate::watch) fn write_status(&mut self) {
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
        if let Err(error) = write_atomically(
            &super::status_path(&self.db_path, &self.runtime_dir),
            &status,
        ) {
            if super::runtime::permission_denied(&error) && self.disabled_warning.is_none() {
                self.disabled_warning = Some(disabled_warning(&error));
            }
        }
    }
}

fn fail(entry: &mut Watched, message: String, options: &WatchOptions) {
    entry.state = SourceState::Error;
    entry.last_error = Some(message);
    entry.need_full = true;
    entry.retry_at = Some(Instant::now() + entry.backoff);
    entry.backoff = (entry.backoff * 2).min(options.retry_max);
}

fn write_atomically(path: &Path, status: &WatchStatus) -> Result<()> {
    let temporary = sibling(path, ".tmp");
    let bytes = serde_json::to_vec(status)?;
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

pub(in crate::watch) fn disabled_warning(error: &anyhow::Error) -> String {
    format!(
        "background file watching is disabled in read-only or sandboxed mode: {error:#}"
    )
}
