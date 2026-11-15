use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::event::ModifyKind;
use tempfile::{tempdir, TempDir};

use crate::support::add_source;
use crate::watch::WatchOptions;

use super::super::Watched;
use super::*;

mod drain;

const LABEL: &str = "docs";

fn engine_watching(root: &Path) -> (TempDir, Engine) {
    let data = tempdir().unwrap();
    let mut engine = Engine::new(
        &data.path().join("knowledge.db"),
        WatchOptions::default(),
        Box::new(|| None),
    )
    .unwrap();
    let source = add_source(&engine.store, LABEL, root);
    engine.watched.insert(
        LABEL.to_string(),
        Watched {
            source,
            root: root.to_path_buf(),
            registration: None,
            state: SourceState::Watching,
            reconciled_at: None,
            batches: 0,
            reindexed_files: 0,
            last_error: None,
            skipped: Vec::new(),
            retry_at: None,
            backoff: Duration::ZERO,
            pending: BTreeSet::new(),
            need_full: false,
            first_event: None,
            last_event: None,
        },
    );
    (data, engine)
}

fn deliver(engine: &mut Engine, outcome: notify::Result<notify::Event>) {
    engine.events_tx.send((LABEL.to_string(), outcome)).unwrap();
    engine.drain_events();
}

fn modify(path: PathBuf) -> notify::Result<notify::Event> {
    Ok(notify::Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path))
}
