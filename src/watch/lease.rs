use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::knowledge::lock::{self, FileLock};

use super::engine::Engine;
use super::{read_status, EmbedderLoader, Holder, RunOutcome, WatchOptions};

pub fn run_with_loader(
    db_path: &Path,
    options: &WatchOptions,
    stop: &AtomicBool,
    embedder_loader: EmbedderLoader,
) -> Result<RunOutcome> {
    let lease = match acquire_lease(&lease_path(db_path), options.lease_wait) {
        Ok(Some(lease)) => lease,
        Ok(None) => {
            let pid = read_status(db_path)?.map(|status| status.pid);
            return Ok(RunOutcome::AlreadyRunning { pid });
        }
        Err(error) if super::runtime::permission_denied(&error) => {
            return Ok(RunOutcome::Disabled {
                warning: super::engine::disabled_warning(&error),
            });
        }
        Err(error) => return Err(error),
    };
    let mut engine = Engine::new(db_path, options.clone(), embedder_loader)?;
    let outcome = engine.run_until(stop)?;
    drop(lease);
    Ok(outcome)
}

fn acquire_lease(path: &Path, wait: Duration) -> Result<Option<FileLock>> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(lease) = try_acquire(path)? {
            return Ok(Some(lease));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn holder(db_path: &Path) -> Result<Holder> {
    match lock::is_held(&lease_path(db_path)) {
        Ok(false) => Ok(Holder::NotRunning),
        Ok(true) => {
            let pid = read_status(db_path)?.map(|status| status.pid);
            Ok(Holder::Running { pid })
        }
        Err(error) if super::runtime::permission_denied(&error) => Ok(Holder::Disabled {
            warning: super::engine::disabled_warning(&error),
        }),
        Err(error) => Err(error),
    }
}

pub(super) fn lease_path(db_path: &Path) -> PathBuf {
    super::sibling(db_path, ".watch.lock")
}

fn try_acquire(path: &Path) -> Result<Option<FileLock>> {
    #[cfg(test)]
    if LEASE_PERMISSION_FAIL.with(|failure| failure.replace(false)) {
        return Err(anyhow::Error::new(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
    }
    FileLock::try_acquire(path)
}

#[cfg(test)]
thread_local! {
    pub(crate) static LEASE_PERMISSION_FAIL: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}
