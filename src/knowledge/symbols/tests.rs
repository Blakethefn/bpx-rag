use super::*;
use crate::knowledge::sync::sync_source;
use crate::knowledge::{open_in_memory, KnowledgeSourceInfo};
use rusqlite::params;
use std::path::Path;

mod lookup;
mod maintain;
mod pending;

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, label: &str, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
    store.get_source(label).unwrap().unwrap()
}

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn exact(store: &KnowledgeStore, name: &str) -> Vec<SymbolHit> {
    lookup_with(store, name, MatchMode::Exact, None, 20).hits
}

fn lookup_with(
    store: &KnowledgeStore,
    name: &str,
    mode: MatchMode,
    source: Option<&str>,
    limit: usize,
) -> SymbolReport {
    lookup(
        store,
        &SymbolQuery {
            name,
            mode,
            source,
            limit,
        },
    )
    .unwrap()
}

fn hit(
    label: &str,
    rel_path: &str,
    name: &str,
    kind: &str,
    container: Option<&str>,
    lines: (u32, u32),
) -> SymbolHit {
    SymbolHit {
        source_label: label.to_string(),
        rel_path: rel_path.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        container: container.map(str::to_string),
        line_start: lines.0,
        line_end: lines.1,
    }
}

fn count(store: &KnowledgeStore, sql: &str, label: &str) -> i64 {
    store
        .conn()
        .query_row(sql, params![label], |row| row.get(0))
        .unwrap()
}
