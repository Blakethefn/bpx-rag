use std::path::Path;
use std::sync::mpsc::Sender;

#[cfg(test)]
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::sync::Arc;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::super::Event;

pub(in super::super) fn register(
    label: &str,
    root: &Path,
    events: Sender<Event>,
) -> Result<RecommendedWatcher> {
    register_failpoint()?;
    let label = label.to_string();
    let mut watcher = notify::recommended_watcher(move |outcome| {
        let _ = events.send((label.clone(), outcome));
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[cfg(test)]
pub(crate) struct RegisterProbe {
    pub(crate) fail_first: std::sync::atomic::AtomicU32,
    pub(crate) attempts: std::sync::Mutex<Vec<std::time::Instant>>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static REGISTER_PROBE: std::cell::RefCell<Option<Arc<RegisterProbe>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn register_failpoint() -> Result<()> {
    let Some(probe) = REGISTER_PROBE.with(|probe| probe.borrow().clone()) else {
        return Ok(());
    };
    probe.attempts.lock().unwrap().push(std::time::Instant::now());
    let failing = probe
        .fail_first
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok();
    if failing {
        anyhow::bail!("injected registration failure: Too many open files (os error 24)");
    }
    Ok(())
}

#[cfg(not(test))]
fn register_failpoint() -> Result<()> {
    Ok(())
}
