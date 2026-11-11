use crate::knowledge::sync;
use rusqlite::params;
use tempfile::tempdir;

// ── Invariant 4: Idempotent sync ───────────────────────────────────────
// "Running sync twice with no changes produces identical state."

#[test]
fn idempotent_sync_no_changes() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();

    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("s{i}.md")),
            format!("# Stable {i}\n\nUnchanging content."),
        )
        .unwrap();
    }

    store
        .add_source("stable", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("stable").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let source = store.get_source("stable").unwrap().unwrap();
    let chunks_after_first: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();

    // Second sync — no file changes
    let r2 = sync::sync_source(&store, &source, None).unwrap();
    assert_eq!(r2.files_unchanged, 5);
    assert_eq!(r2.files_added, 0);
    assert_eq!(r2.files_updated, 0);
    assert_eq!(r2.files_removed, 0);

    let chunks_after_second: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        chunks_after_first, chunks_after_second,
        "chunk count should be identical after idempotent sync"
    );
}
