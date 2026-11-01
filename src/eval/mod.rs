use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self};
use std::path::Path;
use std::time::Duration;

mod embeddings;
mod pool;
mod retrieval;
mod scaling;
pub use embeddings::*;
pub use pool::*;
pub use retrieval::*;
pub use scaling::*;

const TOP_K: usize = 10;
/// Depth of the diagnostic ranking kept per query for pool-based relation studies.
const DIAGNOSTIC_DEPTH: usize = 30;

fn sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read evaluation file {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn corpus_manifest_sha256(corpus_root: &Path) -> Result<String> {
    let mut files = crate::knowledge::sync::walk_source_dir(corpus_root, &[])?
        .into_iter()
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut manifest = Sha256::new();
    for (relative_path, absolute_path) in files {
        let content = fs::read(&absolute_path)
            .with_context(|| format!("failed to fingerprint {}", absolute_path.display()))?;
        manifest.update((relative_path.len() as u64).to_be_bytes());
        manifest.update(relative_path.as_bytes());
        manifest.update(Sha256::digest(content));
    }
    Ok(format!("{:x}", manifest.finalize()))
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read evaluation data {}", path.display()))?;
    parse_jsonl(&contents, path)
}

fn parse_jsonl<T: for<'de> Deserialize<'de>>(contents: &str, path: &Path) -> Result<Vec<T>> {
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .with_context(|| format!("invalid JSONL record in {}", path.display()))
        })
        .collect()
}

fn in_memory_database_bytes(conn: &rusqlite::Connection) -> Result<u64> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(page_count.max(0) as u64 * page_size.max(0) as u64)
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * percentile).ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn duration_cmp(left: &Duration, right: &Duration) -> std::cmp::Ordering {
    left.cmp(right)
}

#[cfg(test)]
mod tests;
