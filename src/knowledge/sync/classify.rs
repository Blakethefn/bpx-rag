use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::chunk::evidence::{EntryObservation, Reference};
use crate::chunk::{chunk_content_for_path, chunker_version_for_path, Symbol};

use super::files::{sha256_hex, stat_before_read, DbFile, StatRecord};
use super::policy::contains_google_api_key;
use super::SkippedFile;

/// What the walk saw, reported in the sync result.
pub(super) struct ScanCounts {
    pub(super) files_scanned: usize,
    pub(super) sensitive_files_skipped: usize,
    pub(super) skipped: Vec<SkippedFile>,
}

/// Changes decided by classification, applied in one transaction.
#[derive(Default)]
pub(super) struct ChangePlan {
    /// (relative path, absolute path, content hash, stat)
    pub(super) to_add: Vec<(String, PathBuf, String, StatRecord)>,
    /// (file id, absolute path, content hash, stat)
    pub(super) to_update: Vec<(i64, PathBuf, String, StatRecord)>,
    pub(super) to_remove: Vec<i64>,
    /// Unchanged files whose symbol or reference stamp is stale. Evidence comes from the content
    /// read during classification, not from a later disk read.
    pub(super) backfill: Vec<BackfillEvidence>,
    /// Unchanged files whose modification time was never recorded.
    pub(super) backfill_stat: Vec<(i64, StatRecord)>,
    pub(super) unchanged: usize,
}

pub(super) struct BackfillEvidence {
    pub(super) file_id: i64,
    pub(super) rel_path: String,
    pub(super) status: &'static str,
    pub(super) symbols: Vec<Symbol>,
    pub(super) references: Vec<Reference>,
    pub(super) entries: Vec<EntryObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Classification {
    Indexed,
    Sensitive,
    /// Not valid UTF-8. The file is skipped with this reason, and any rows it had are removed.
    Skipped(String),
}

/// Record a file that was not indexed and must lose any rows it had.
pub(super) fn mark_excluded(
    db_files: &HashMap<String, DbFile>,
    rel_path: &str,
    plan: &mut ChangePlan,
) {
    if let Some(db_file) = db_files.get(rel_path) {
        plan.to_remove.push(db_file.id);
    }
}

/// Read one included file, decide whether it is new, changed, or unchanged, and record any
/// backfill it needs. The stat is taken before the content is read.
pub(super) fn classify_file(
    rel_path: &str,
    abs_path: &Path,
    db_files: &HashMap<String, DbFile>,
    plan: &mut ChangePlan,
) -> Result<Classification> {
    // A file that cannot be inspected or read (permission denied, an I/O error, or removed
    // since the walk) is a skip with a reason, like invalid UTF-8, so one bad file cannot stop
    // the other files from syncing. Its indexed rows are removed, because the index could no
    // longer be checked against the file.
    let stat = match stat_before_read(abs_path) {
        Ok(stat) => stat,
        Err(error) => {
            return Ok(Classification::Skipped(format!(
                "unreadable: {}",
                error.root_cause()
            )))
        }
    };
    let bytes = match fs::read(abs_path) {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Classification::Skipped(format!("unreadable: {error}"))),
    };

    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) => return Ok(Classification::Skipped(format!("not valid UTF-8: {error}"))),
    };
    if contains_google_api_key(&content) {
        return Ok(Classification::Sensitive);
    }
    let hash = sha256_hex(content.as_bytes());

    let chunker_version = chunker_version_for_path(Path::new(rel_path));
    match db_files.get(rel_path) {
        Some(db_file) if db_file.hash != hash || db_file.chunker_version != chunker_version => {
            plan.to_update
                .push((db_file.id, abs_path.to_path_buf(), hash, stat));
        }
        Some(db_file) => {
            plan.unchanged += 1;
            // The content matches, but its symbol or reference rows may come from an older
            // writer. `evidence_current` compares both stamps with the stored hash.
            if !db_file.evidence_current {
                let evidence = chunk_content_for_path(Path::new(rel_path), &content);
                plan.backfill.push(BackfillEvidence {
                    file_id: db_file.id,
                    rel_path: rel_path.to_string(),
                    status: evidence.symbol_status,
                    symbols: evidence.symbols,
                    references: evidence.references,
                    entries: evidence.entries,
                });
            }
            if !db_file.stat_recorded {
                plan.backfill_stat.push((db_file.id, stat));
            }
        }
        None => plan
            .to_add
            .push((rel_path.to_string(), abs_path.to_path_buf(), hash, stat)),
    }
    Ok(Classification::Indexed)
}
