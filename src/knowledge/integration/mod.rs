use crate::knowledge::*;
use crate::support::{make_store, make_store_with_embedder};

mod enrichment;
mod scope;
mod search;
mod sync;

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

// ── Test Anchor 1: Incremental sync correctness ────────────────────────
// "Create temp dir with 10 files. Sync. Modify 2, delete 1, add 1.
//  Sync. Verify counts and content."
