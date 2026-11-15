use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

use crate::knowledge::enrichment::build_enrichment_fn;
use crate::knowledge::lock::{writer_lock_path, FileLock};
use crate::knowledge::sync::{sync_paths, sync_source, EnrichmentFn, SyncResult};
use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

use super::super::super::SourceState;
use super::super::{fail, Engine};

impl Engine {
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

pub(in super::super) fn locked_sync(
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
        let prefix = format!("{relative}/");
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
