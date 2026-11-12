use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};
use anyhow::Result;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::time::Instant;

#[cfg(test)]
use rusqlite::params;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::PathBuf;

mod classify;
mod commit;
mod files;
mod index;
mod plan;
mod policy;
mod scrub;
mod walk;
use classify::{classify_file, mark_excluded, ChangePlan, Classification, ScanCounts};
use files::*;
use index::delete_file_chunks;
use plan::apply_plan;
use policy::{contains_google_api_key, is_sensitive_relative_path};
pub use scrub::*;
pub(crate) use walk::{walk_source_dir, walk_source_dir_with_report};

pub type EnrichmentFn<'a> = Option<&'a dyn Fn(&str, &Path) -> serde_json::Value>;

#[derive(Debug, serde::Serialize)]
pub struct SyncResult {
    pub source_label: String,
    pub files_scanned: usize,
    pub sensitive_files_skipped: usize,
    pub skipped_files: Vec<SkippedFile>,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_unchanged: usize,
    pub chunks_total: usize,
    pub chunks_indexed: usize,
    pub tree_sitter_chunks_indexed: usize,
    pub existing_chunker_chunks_indexed: usize,
    pub tree_sitter_files_indexed: usize,
    pub existing_chunker_files_indexed: usize,
    pub links_created: usize,
    pub embedding_ms: u64,
    pub embedding_calls: usize,
    pub embedding_chunks: usize,
    pub transaction_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedFile {
    pub rel_path: String,
    pub reason: String,
}

pub fn sync_source(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: EnrichmentFn<'_>,
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let base_path = Path::new(&source.path);

    let walk = walk_source_dir_with_report(base_path, &source.patterns)?;
    let disk_files = walk.files;
    let mut sensitive_files_skipped = walk.sensitive_files_skipped;

    let db_files = load_db_files(conn, source.id)?;

    let mut plan = ChangePlan::default();
    let mut excluded_content_paths = HashSet::new();
    let mut skipped = walk.unreadable;
    for (rel_path, abs_path) in &disk_files {
        match classify_file(rel_path, abs_path, &db_files, &mut plan)? {
            Classification::Indexed => {}
            Classification::Sensitive => {
                sensitive_files_skipped += 1;
                excluded_content_paths.insert(rel_path.clone());
            }
            Classification::Skipped(reason) => {
                skipped.push(SkippedFile {
                    rel_path: rel_path.clone(),
                    reason,
                });
                excluded_content_paths.insert(rel_path.clone());
            }
        }
    }
    for (rel_path, db_file) in &db_files {
        if !disk_files.contains_key(rel_path) || excluded_content_paths.contains(rel_path) {
            plan.to_remove.push(db_file.id);
        }
    }

    let counts = ScanCounts {
        files_scanned: disk_files.len(),
        sensitive_files_skipped,
        skipped,
    };
    apply_plan(store, source, &db_files, plan, enrichment_fn, start, counts)
}

pub fn sync_paths(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: EnrichmentFn<'_>,
    rel_paths: &[String],
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let base_path = Path::new(&source.path);

    let walk = walk_source_dir_with_report(base_path, &source.patterns)?;
    let files_scanned = walk.files.len();
    let mut sensitive_files_skipped = walk.sensitive_files_skipped;
    let db_files = load_db_files(conn, source.id)?;

    let mut plan = ChangePlan::default();
    let unique_paths: BTreeSet<&String> = rel_paths.iter().collect();
    let mut skipped: Vec<SkippedFile> = walk
        .unreadable
        .iter()
        .filter(|file| unique_paths.contains(&file.rel_path))
        .cloned()
        .collect();
    for rel_path in unique_paths {
        match walk.files.get(rel_path) {
            Some(abs_path) => match classify_file(rel_path, abs_path, &db_files, &mut plan)? {
                Classification::Indexed => {}
                Classification::Sensitive => {
                    sensitive_files_skipped += 1;
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
                Classification::Skipped(reason) => {
                    skipped.push(SkippedFile {
                        rel_path: rel_path.clone(),
                        reason,
                    });
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
            },
            None => mark_excluded(&db_files, rel_path, &mut plan),
        }
    }

    let counts = ScanCounts {
        files_scanned,
        sensitive_files_skipped,
        skipped,
    };
    apply_plan(store, source, &db_files, plan, enrichment_fn, start, counts)
}

#[cfg(test)]
mod tests;
