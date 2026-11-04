//! Cross-process exclusive file locks, used for the writer lock and the watcher lease.
//!
//! The lock is an advisory `flock` taken with the standard library. The kernel releases it when
//! the holding process closes the file or exits, so a crashed holder never leaves a stale lock.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// An exclusive lock on a file, released when dropped or when the process exits.
#[derive(Debug)]
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Block until the lock is free, then hold it.
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = open_lock_file(path)?;
        file.lock()
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(Self { file })
    }

    /// Hold the lock if it is free. Returns `None` while another holder has it.
    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let file = open_lock_file(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => {
                Err(error).with_context(|| format!("failed to lock {}", path.display()))
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn open_lock_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open lock file {}", path.display()))
}

/// The writer lock sits beside the database: `knowledge.db` gives `knowledge.db.write.lock`.
pub fn writer_lock_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(".write.lock");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests;
