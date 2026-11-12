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
