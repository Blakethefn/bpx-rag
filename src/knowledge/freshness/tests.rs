use super::*;
use crate::knowledge::sync::sync_source;
use crate::knowledge::KnowledgeSourceInfo;
use rusqlite::params;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod check;
mod latency;
mod sync;

const HOUR: Duration = Duration::from_secs(3600);

fn ns(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

fn now_ns() -> i64 {
    ns(SystemTime::now())
}

fn set_mtime(path: &Path, time: SystemTime) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn sync_dir(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source("test", dir.to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
    source
}

fn write_aged(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    set_mtime(&path, SystemTime::now() - HOUR);
    path
}

fn record(store: &KnowledgeStore, rel_path: &str) -> IndexedFile {
    store
        .conn()
        .query_row(
            "SELECT rel_path, content_hash, size_bytes, mtime_ns, stat_checked_ns
             FROM knowledge_files WHERE rel_path = ?1",
            params![rel_path],
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
        .unwrap()
}

fn check(store: &KnowledgeStore, dir: &Path, rel_path: &str) -> Freshness {
    check_indexed_file(dir, &record(store, rel_path), now_ns())
}

fn freshness(state: FreshnessState, basis: FreshnessBasis) -> Freshness {
    Freshness { state, basis }
}
