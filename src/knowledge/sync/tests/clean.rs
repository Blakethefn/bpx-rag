use super::super::*;
use super::{make_store, register_source};
use tempfile::tempdir;

// ── FTS5 cleanup ────────────────────────────────────────────────────────

#[test]
fn no_orphaned_chunks_after_file_removal() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("keep.md"), "# Keep\nPersistent").unwrap();
    fs::write(dir.path().join("gone.md"), "# Gone\nWill be deleted").unwrap();

    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    fs::remove_file(dir.path().join("gone.md")).unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    sync_source(&store, &source, None).unwrap();

    // No orphaned chunk_meta (meta pointing to deleted files)
    let orphaned: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta kcm
             LEFT JOIN knowledge_files kf ON kcm.file_id = kf.id
             WHERE kf.id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphaned, 0, "no orphaned chunk_meta");

    // Chunk count in knowledge_chunks matches chunk_meta count
    let fts_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunks", [], |row| {
            row.get(0)
        })
        .unwrap();
    let meta_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunk_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fts_count, meta_count, "FTS5 and meta counts should match");
}
