use super::*;
use crate::embed::tests::MockEmbedder;
use std::sync::Arc;

#[test]
fn backfill_embeds_every_pending_chunk_across_batches_and_reports_counts() {
    let mut fixture = Fixture::new(2);
    let sources = tempdir().unwrap();
    write_files(sources.path(), 3);
    add_synced_source(&fixture.app.knowledge, "docs", sources.path());
    let total = chunk_count(fixture.conn());
    assert!(
        total > 2,
        "fixture must span more than one batch, got {total}"
    );
    assert_eq!(embedding_count(fixture.conn()), 0);
    fixture
        .app
        .knowledge
        .set_embedder(Arc::new(MockEmbedder::new()));

    let result = fixture.app.backfill_embeddings(None).unwrap();

    assert_eq!(result, report(None, total, 0, 0));
    assert_eq!(embedding_count(fixture.conn()), total);
}

#[test]
fn backfill_with_no_pending_chunks_writes_nothing() {
    let mut fixture = Fixture::new(2);
    fixture
        .app
        .knowledge
        .set_embedder(Arc::new(MockEmbedder::new()));
    let sources = tempdir().unwrap();
    write_files(sources.path(), 3);
    add_synced_source(&fixture.app.knowledge, "docs", sources.path());
    let total = chunk_count(fixture.conn());
    assert_eq!(
        embedding_count(fixture.conn()),
        total,
        "sync with an embedder should leave nothing pending"
    );

    let result = fixture.app.backfill_embeddings(None).unwrap();

    assert_eq!(result, report(None, 0, 0, 0));
    assert_eq!(embedding_count(fixture.conn()), total);
}
