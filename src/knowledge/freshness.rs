//! Freshness of retrieved results against the files they were indexed from.
//!
//! Metadata (size and mtime) is the cheap first check. A content hash is taken only when the
//! metadata differs, when it is too close to the stat time to be trusted (a racy record), or when
//! the mtime lies in the future. A query never writes.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use super::query::KnowledgeSearchResult;
use super::KnowledgeStore;

/// Filesystems can store mtimes with coarse granularity (FAT: 2 s). A record taken within this
/// margin of the mtime cannot prove that a later write changed nothing.
const RACY_MARGIN_NS: i64 = 2_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessState {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessBasis {
    /// Size and mtime matched the index, and the record was not racy.
    Metadata,
    /// The content hash was compared.
    Hash,
    /// The indexed file no longer exists.
    Missing,
    /// The content hash differs from the index.
    Changed,
    /// The file exists but could not be read.
    Unreadable,
    /// The source directory cannot be read.
    SourceUnavailable,
    /// The index has no metadata for this file. A sync records it.
    NotRecorded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    pub state: FreshnessState,
    pub basis: FreshnessBasis,
}

impl Freshness {
    pub fn label(&self) -> String {
        let state = match self.state {
            FreshnessState::Fresh => "fresh",
            FreshnessState::Stale => "stale",
            FreshnessState::Unknown => "unknown",
        };
        let basis = match self.basis {
            // Not a content check: size and mtime agree, but edits that keep both are missed.
            FreshnessBasis::Metadata => "metadata match, content not hashed",
            FreshnessBasis::Hash => "hash",
            FreshnessBasis::Missing => "missing",
            FreshnessBasis::Changed => "changed",
            FreshnessBasis::Unreadable => "unreadable",
            FreshnessBasis::SourceUnavailable => "source unavailable",
            FreshnessBasis::NotRecorded => "not recorded",
        };
        format!("{state} ({basis})")
    }
}

/// What the index recorded for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub rel_path: String,
    pub content_hash: String,
    pub size_bytes: i64,
    pub mtime_ns: Option<i64>,
    pub stat_checked_ns: Option<i64>,
}

pub fn check_indexed_file(base: &Path, record: &IndexedFile, now_ns: i64) -> Freshness {
    if !fs::metadata(base).is_ok_and(|meta| meta.is_dir()) {
        return unknown(FreshnessBasis::SourceUnavailable);
    }
    let absolute = base.join(&record.rel_path);
    let metadata = match fs::metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return stale(FreshnessBasis::Missing)
        }
        Err(_) => return unknown(FreshnessBasis::Unreadable),
    };
    let (Some(indexed_mtime), Some(checked_ns)) = (record.mtime_ns, record.stat_checked_ns) else {
        return unknown(FreshnessBasis::NotRecorded);
    };
    let Some(current_mtime) = metadata.modified().ok().map(system_time_ns) else {
        return verify_by_hash(&absolute, record);
    };
    let unchanged_metadata =
        metadata.len() as i64 == record.size_bytes && current_mtime == indexed_mtime;
    let racy =
        indexed_mtime > checked_ns - RACY_MARGIN_NS || indexed_mtime > now_ns + RACY_MARGIN_NS;
    if unchanged_metadata && !racy {
        return fresh(FreshnessBasis::Metadata);
    }
    verify_by_hash(&absolute, record)
}

fn verify_by_hash(absolute: &Path, record: &IndexedFile) -> Freshness {
    match fs::read(absolute) {
        Ok(bytes) => {
            let digest = format!("{:x}", Sha256::digest(&bytes));
            if digest == record.content_hash {
                fresh(FreshnessBasis::Hash)
            } else {
                stale(FreshnessBasis::Changed)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => stale(FreshnessBasis::Missing),
        Err(_) => unknown(FreshnessBasis::Unreadable),
    }
}

/// Check the files behind the returned results, in result order. Each result is checked once.
pub fn check_results(
    store: &KnowledgeStore,
    results: &[KnowledgeSearchResult],
) -> Result<Vec<Freshness>> {
    let now = now_ns();
    let mut source_paths: HashMap<i64, Option<String>> = HashMap::new();
    let mut checks = Vec::with_capacity(results.len());
    for result in results {
        let base = match source_paths.get(&result.source_id) {
            Some(path) => path.clone(),
            None => {
                let path = store
                    .conn()
                    .query_row(
                        "SELECT path FROM knowledge_sources WHERE id = ?1",
                        params![result.source_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                source_paths.insert(result.source_id, path.clone());
                path
            }
        };
        let Some(base) = base else {
            checks.push(unknown(FreshnessBasis::SourceUnavailable));
            continue;
        };
        let record = indexed_file(store, result.file_id)?;
        checks.push(match record {
            Some(record) => check_indexed_file(Path::new(&base), &record, now),
            None => unknown(FreshnessBasis::NotRecorded),
        });
    }
    Ok(checks)
}

fn indexed_file(store: &KnowledgeStore, file_id: i64) -> Result<Option<IndexedFile>> {
    let record = store
        .conn()
        .query_row(
            "SELECT rel_path, content_hash, size_bytes, mtime_ns, stat_checked_ns
             FROM knowledge_files WHERE id = ?1",
            params![file_id],
            |row| {
                Ok(IndexedFile {
                    rel_path: row.get(0)?,
                    content_hash: row.get(1)?,
                    size_bytes: row.get(2)?,
                    mtime_ns: row.get(3)?,
                    stat_checked_ns: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(record)
}

/// Nanoseconds since the Unix epoch. Times before the epoch are negative.
pub(crate) fn system_time_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_nanos() as i64,
        Err(before) => -(before.duration().as_nanos() as i64),
    }
}

pub(crate) fn now_ns() -> i64 {
    system_time_ns(SystemTime::now())
}

fn fresh(basis: FreshnessBasis) -> Freshness {
    Freshness {
        state: FreshnessState::Fresh,
        basis,
    }
}

fn stale(basis: FreshnessBasis) -> Freshness {
    Freshness {
        state: FreshnessState::Stale,
        basis,
    }
}

fn unknown(basis: FreshnessBasis) -> Freshness {
    Freshness {
        state: FreshnessState::Unknown,
        basis,
    }
}

#[cfg(test)]
mod tests;
