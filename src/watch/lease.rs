use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::knowledge::lock::FileLock;

use super::engine::Engine;
use super::{read_status, sibling, EmbedderLoader, Holder, RunOutcome, WatchOptions};

pub fn run_with_loader(
    db_path: &Path,
    options: &WatchOptions,
    stop: &AtomicBool,
    embedder_loader: EmbedderLoader,
) -> Result<RunOutcome> {
    let Some(lease) = acquire_lease(&lease_path(db_path), options.lease_wait)? else {
        let pid = read_status(db_path)?.map(|status| status.pid);
        return Ok(RunOutcome::AlreadyRunning { pid });
    };
    let mut engine = Engine::new(db_path, options.clone(), embedder_loader)?;
    engine.run_until(stop)?;
    drop(lease);
    Ok(RunOutcome::Stopped)
}

fn acquire_lease(path: &Path, wait: Duration) -> Result<Option<FileLock>> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(lease) = FileLock::try_acquire(path)? {
            return Ok(Some(lease));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn holder(db_path: &Path) -> Result<Holder> {
    match FileLock::try_acquire(&lease_path(db_path))? {
        Some(lock) => {
            drop(lock);
            Ok(Holder::NotRunning)
        }
        None => {
            let pid = read_status(db_path)?.map(|status| status.pid);
            Ok(Holder::Running { pid })
        }
    }
}

pub(super) fn lease_path(db_path: &Path) -> PathBuf {
    sibling(db_path, ".watch.lock")
}
