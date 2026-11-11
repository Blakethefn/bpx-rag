use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Instant;

#[cfg(test)]
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::sync::Arc;

use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::knowledge::enrichment::build_enrichment_fn;
use crate::knowledge::lock::{writer_lock_path, FileLock};
use crate::knowledge::sync::{sync_paths, sync_source, EnrichmentFn, SyncResult};
use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

use super::super::{SourceState, EVENT_POLL};
use super::{fail, Engine, Event};

impl Engine {
    /// Move queued events into each source's pending set. A notify error or an overflow asks for a
    /// full reconcile instead, because the lost events cannot be recovered.
    pub(super) fn drain_events(&mut self) {
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

    pub(super) fn flush_due_batches(&mut self) {
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

    pub(in crate::watch) fn process_batch(&mut self, label: &str) {
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
}

pub(super) fn register(label: &str, root: &Path, events: Sender<Event>) -> Result<RecommendedWatcher> {
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

/// Run `operation` while holding the writer lock. Manual sync takes the same lock.
pub(super) fn locked_sync(
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
