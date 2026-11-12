use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug)]
pub struct FileLock {
    file: File,
}

impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = open_lock_file(path)?;
        file.lock()
            .with_context(|| format!("failed to lock {}", path.display()))?;
        Ok(Self { file })
    }

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

pub fn writer_lock_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(".write.lock");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests;
