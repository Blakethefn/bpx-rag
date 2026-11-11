use crate::knowledge::sync;
use tempfile::tempdir;

// ── Sync with embedder generates searchable vectors ────────────────────

#[test]
fn sync_with_embedder_produces_embeddings() {
    let store = super::super::make_store_with_embedder();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("embed.md"),
        "# Embedding Test\n\nVector embeddings for semantic search.",
    )
    .unwrap();

    store
        .add_source("emb", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("emb").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let embed_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        embed_count > 0,
        "embeddings should be generated during sync"
    );

    // Every chunk should have a corresponding embedding
    let chunk_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunk_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        chunk_count, embed_count,
        "every chunk should have an embedding"
    );
}
