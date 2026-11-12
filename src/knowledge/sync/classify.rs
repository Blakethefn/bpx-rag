use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::chunk::evidence::{EntryObservation, Reference};
use crate::chunk::{chunk_content_for_path, chunker_version_for_path, Symbol};

use super::files::{sha256_hex, stat_before_read, DbFile, StatRecord};
use super::policy::contains_google_api_key;
use super::SkippedFile;

pub(super) struct ScanCounts {
    pub(super) files_scanned: usize,
    pub(super) sensitive_files_skipped: usize,
    pub(super) skipped: Vec<SkippedFile>,
}

#[derive(Default)]
pub(super) struct ChangePlan {
    pub(super) to_add: Vec<(String, PathBuf, String, StatRecord)>,
    pub(super) to_update: Vec<(i64, PathBuf, String, StatRecord)>,
    pub(super) to_remove: Vec<i64>,
    pub(super) backfill: Vec<BackfillEvidence>,
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
    Skipped(String),
}

pub(super) fn mark_excluded(
    db_files: &HashMap<String, DbFile>,
    rel_path: &str,
    plan: &mut ChangePlan,
) {
    if let Some(db_file) = db_files.get(rel_path) {
        plan.to_remove.push(db_file.id);
    }
}

pub(super) fn classify_file(
    rel_path: &str,
    abs_path: &Path,
    db_files: &HashMap<String, DbFile>,
    plan: &mut ChangePlan,
) -> Result<Classification> {
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
