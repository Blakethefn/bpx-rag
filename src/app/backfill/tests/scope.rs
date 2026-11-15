use super::*;
use crate::embed::tests::MockEmbedder;
use std::sync::Arc;

#[test]
fn source_scoped_backfill_embeds_only_that_source_and_counts_only_its_remainder() {
    let mut fixture = Fixture::new(16);
    let alpha_dir = tempdir().unwrap();
    let beta_dir = tempdir().unwrap();
    write_files(alpha_dir.path(), 2);
    write_files(beta_dir.path(), 3);
    let alpha = add_synced_source(&fixture.app.knowledge, "alpha", alpha_dir.path());
    let beta = add_synced_source(&fixture.app.knowledge, "beta", beta_dir.path());
    let alpha_total = source_chunk_count(fixture.conn(), alpha.id);
    let beta_total = source_chunk_count(fixture.conn(), beta.id);
    assert!(alpha_total > 0 && beta_total > 0);
    fixture
        .app
        .knowledge
        .set_embedder(Arc::new(MockEmbedder::new()));

    let result = fixture.app.backfill_embeddings(Some("alpha")).unwrap();

    assert_eq!(result, report(Some("alpha"), alpha_total, 0, 0));
    assert_eq!(
        source_embedding_count(fixture.conn(), alpha.id),
        alpha_total
    );
    assert_eq!(source_embedding_count(fixture.conn(), beta.id), 0);
}
