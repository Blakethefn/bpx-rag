use super::super::*;
use super::{
    chunk_count_for_source, file_count_for_source, make_store, make_store_with_embedder,
    register_source, CountingEmbedder, FailingEmbedder, MutatingEmbedder,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;


#[test]
fn sync_generates_embeddings_when_embedder_present() {
    let store = make_store_with_embedder();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("test.md"),
        "# Embedding Test\nContent for embedding generation",
    )
    .unwrap();

    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(count > 0, "embeddings should be generated");
}

#[test]
fn sync_embeds_all_changed_files_in_one_corpus_call_before_persisting() {
    let mut store = make_store();
    let calls = Arc::new(AtomicUsize::new(0));
    let embedded_texts = Arc::new(AtomicUsize::new(0));
    store.set_embedder(Arc::new(CountingEmbedder {
        calls: Arc::clone(&calls),
        embedded_texts: Arc::clone(&embedded_texts),
    }));
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("first.md"), "# First\nfirst document").unwrap();
    fs::write(dir.path().join("second.md"), "# Second\nsecond document").unwrap();
    let source = register_source(&store, dir.path());

    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.embedding_calls, 1);
    assert_eq!(
        result.embedding_chunks,
        embedded_texts.load(Ordering::SeqCst)
    );
    assert_eq!(result.embedding_chunks, result.chunks_indexed);
    assert!(result.transaction_ms <= result.duration_ms);
    let persisted_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(persisted_count as usize, result.embedding_chunks);
}

#[test]
fn embedding_failure_happens_before_the_sync_transaction_and_preserves_index() {
    let mut store = make_store();
    let dir = tempdir().unwrap();
    let existing_path = dir.path().join("existing.md");
    fs::write(&existing_path, "# Original\nExisting indexed content").unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    fs::write(&existing_path, "# Changed\nNew replacement content").unwrap();
    fs::write(dir.path().join("new.md"), "# New file\nNew indexed content").unwrap();
    store.set_embedder(Arc::new(FailingEmbedder));

    let error = sync_source(&store, &source, None).unwrap_err();

    assert!(error
        .to_string()
        .contains("failed to embed changed corpus chunks"));
    assert!(store.conn().is_autocommit());
    assert_eq!(file_count_for_source(&store, source.id), 1);
    assert_eq!(chunk_count_for_source(&store, source.id), 1);
    let retained_content: String = store
        .conn()
        .query_row("SELECT content FROM knowledge_chunks", [], |row| row.get(0))
        .unwrap();
    assert!(retained_content.contains("Existing indexed content"));
    assert!(!retained_content.contains("replacement"));
}

#[test]
fn changed_file_during_embedding_aborts_before_stale_content_is_committed() {
    let mut store = make_store();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("existing.md");
    fs::write(&file_path, "# Original\nExisting indexed content").unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    let original_hash: String = store
        .conn()
        .query_row(
            "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();

    fs::write(&file_path, "# Prepared\nContent before model inference").unwrap();
    store.set_embedder(Arc::new(MutatingEmbedder {
        file_path: file_path.clone(),
    }));

    let error = sync_source(&store, &source, None).unwrap_err();

    assert!(error
        .to_string()
        .contains("changed while embeddings were being prepared"));
    assert!(store.conn().is_autocommit());
    let stored_hash: String = store
        .conn()
        .query_row(
            "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_hash, original_hash);
    let stored_content: String = store
        .conn()
        .query_row("SELECT content FROM knowledge_chunks", [], |row| row.get(0))
        .unwrap();
    assert!(stored_content.contains("Existing indexed content"));
    assert!(!stored_content.contains("Prepared"));
    assert_eq!(file_count_for_source(&store, source.id), 1);
}

#[test]
fn sync_works_without_embedder() {
    let store = make_store(); // no embedder
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.md"), "# No Embed\nStill works").unwrap();

    let source = register_source(&store, dir.path());
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 1);
    let embed_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(embed_count, 0, "no embeddings without embedder");
}
