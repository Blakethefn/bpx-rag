use std::path::Path;
use std::time::Instant;

use notify::EventKind;

use super::super::{SourceState, EVENT_POLL};
use super::{Engine, Watched};

mod batch;
mod watcher;

pub(super) use batch::locked_sync;
pub(super) use watcher::register;

#[cfg(test)]
pub(crate) use watcher::{RegisterProbe, REGISTER_PROBE};

impl Engine {
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
                        continue;
                    }
                    if event.need_rescan() {
                        entry.need_full = true;
                    }
                    for path in &event.paths {
                        note_path(entry, path);
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
}

fn note_path(entry: &mut Watched, path: &Path) {
    match path.strip_prefix(&entry.root) {
        Ok(relative) if !relative.as_os_str().is_empty() => {
            entry.pending.insert(normalize_separators(
                relative.to_string_lossy().into_owned(),
            ));
        }
        _ => entry.need_full = true,
    }
}

fn normalize_separators(relative: String) -> String {
    if cfg!(windows) {
        relative.replace('\\', "/")
    } else {
        relative
    }
}

#[cfg(test)]
mod tests;
