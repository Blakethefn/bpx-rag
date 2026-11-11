use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::knowledge::sync::sync_source;

use super::events::{locked_sync, register};
use super::fail;
use super::super::SourceState;
use super::Engine;

impl Engine {
    /// Start watching sources newly switched on, and stop watching sources switched off.
    pub(in crate::watch) fn refresh_sources(&mut self) -> Result<()> {
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
                    super::Watched {
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

    pub(in crate::watch) fn reconcile(&mut self, label: &str) {
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

    pub(in crate::watch) fn retry_due(&mut self) {
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
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
