use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

/// Base directory for all bpcontext data
pub(crate) fn data_dir() -> Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .context("Could not find local data directory")?
        .join("bpcontext");
    Ok(dir)
}

/// Open a SQLite connection with WAL mode and recommended pragmas
pub fn open_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open database at {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(conn)
}

/// List all content DB files with their modification times
#[allow(dead_code)]
pub fn list_content_dbs() -> Result<Vec<(PathBuf, std::time::SystemTime)>> {
    let dir = data_dir()?.join("content");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut dbs = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "db") {
            let modified = entry.metadata()?.modified()?;
            dbs.push((path, modified));
        }
    }
    Ok(dbs)
}

/// Delete content DBs older than the given number of days
#[allow(dead_code)]
pub fn cleanup_stale_dbs(stale_days: u64) -> Result<u32> {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(stale_days * 86400);
    let dbs = list_content_dbs()?;
    let mut deleted = 0u32;
    for (path, modified) in dbs {
        if modified < cutoff {
            // Also remove WAL and SHM files
            let _ = fs::remove_file(path.with_extension("db-wal"));
            let _ = fs::remove_file(path.with_extension("db-shm"));
            fs::remove_file(&path)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Hex encoding helper (avoids adding the hex crate)
mod hex {
}
