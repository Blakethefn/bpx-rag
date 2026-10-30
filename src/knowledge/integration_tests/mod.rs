use crate::knowledge::*;
use std::sync::Arc;

mod enrichment_tests;
mod scope_tests;
mod search_tests;
mod sync_tests;

fn link_count(store: &KnowledgeStore, source_id: i64) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_links
             WHERE source_file_id IN (SELECT id FROM knowledge_files WHERE source_id = ?1)",
            rusqlite::params![source_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

fn make_store_with_embedder() -> KnowledgeStore {
    let mut store = make_store();
    store.set_embedder(Arc::new(crate::embed::tests::MockEmbedder::new()));
    store
}

// ── Test Anchor 1: Incremental sync correctness ────────────────────────
// "Create temp dir with 10 files. Sync. Modify 2, delete 1, add 1.
//  Sync. Verify counts and content."
