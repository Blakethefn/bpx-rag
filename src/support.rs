use crate::knowledge::sync::sync_source;
use crate::knowledge::{open_in_memory, KnowledgeSourceInfo, KnowledgeStore};
use std::path::Path;
use std::sync::Arc;

pub(crate) fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

pub(crate) fn make_store_with_embedder() -> KnowledgeStore {
    let mut store = make_store();
    store.set_embedder(Arc::new(crate::embed::tests::MockEmbedder::new()));
    store
}

pub(crate) fn add_source(store: &KnowledgeStore, label: &str, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
    store.get_source(label).unwrap().unwrap()
}

pub(crate) fn add_synced_source(
    store: &KnowledgeStore,
    label: &str,
    dir: &Path,
) -> KnowledgeSourceInfo {
    let source = add_source(store, label, dir);
    sync_source(store, &source, None).unwrap();
    source
}
