use super::super::*;
use super::CountingEmbedder;
use crate::support::{add_source, make_store};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

// ── identifier index upgrade ─────────────────────────────────────────────

#[test]
fn upgrading_the_identifier_index_does_not_reembed_unchanged_files() {
    let mut store = make_store();
    let calls = Arc::new(AtomicUsize::new(0));
    let embedded_texts = Arc::new(AtomicUsize::new(0));
    store.set_embedder(Arc::new(CountingEmbedder {
        calls: Arc::clone(&calls),
        embedded_texts: Arc::clone(&embedded_texts),
    }));
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("users.ts"),
        "export function getUserById(id) { return id; }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("notes.md"),
        "# Notes\nblue-green-deploy plan\n",
    )
    .unwrap();
    let source = add_source(&store, "test", dir.path());
    let first = sync_source(&store, &source, None).unwrap();
    assert_eq!(first.files_added, 2);
    let embed_calls_after_index = calls.load(Ordering::SeqCst);

    // Simulate a database from before the identifier index existed.
    store
        .conn()
        .execute_batch("DELETE FROM knowledge_identifiers; DROP TABLE knowledge_index_meta;")
        .unwrap();
    crate::knowledge::schema::apply_schema(store.conn()).unwrap();

    let second = sync_source(&store, &source, None).unwrap();

    assert_eq!(
        second.files_updated, 0,
        "unchanged files must not be reindexed"
    );
    assert_eq!(second.files_unchanged, 2);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        embed_calls_after_index,
        "no embedding call may be made for an identifier-index upgrade"
    );
    let identifier_rows: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_identifiers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        identifier_rows, 2,
        "backfill must restore one row per chunk"
    );
}
