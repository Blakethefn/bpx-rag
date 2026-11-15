use super::*;
use crate::embed::tests::MockEmbedder;
use crate::embed::Embed;
use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct MisshapenEmbedder {
    inner: MockEmbedder,
    bad_index: usize,
}

impl Embed for MisshapenEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed_one(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut vectors = self.inner.embed_batch(texts)?;
        if let Some(vector) = vectors.get_mut(self.bad_index) {
            vector.push(0.0);
        }
        Ok(vectors)
    }

    fn dim(&self) -> usize {
        self.inner.dim()
    }
}

struct EditingEmbedder {
    inner: MockEmbedder,
    db_path: PathBuf,
    edited: AtomicBool,
}

impl Embed for EditingEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        self.inner.embed_one(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if !self.edited.swap(true, Ordering::SeqCst) {
            let other = Connection::open(&self.db_path)?;
            other.execute(
                "UPDATE knowledge_chunks SET content = content || ' edited'
                 WHERE rowid = (SELECT MIN(rowid) FROM knowledge_chunks)",
                [],
            )?;
        }
        self.inner.embed_batch(texts)
    }

    fn dim(&self) -> usize {
        self.inner.dim()
    }
}

#[test]
fn chunk_edited_before_its_write_is_skipped_then_embedded_in_a_later_batch() {
    let mut fixture = Fixture::new(2);
    let sources = tempdir().unwrap();
    write_files(sources.path(), 3);
    add_synced_source(&fixture.app.knowledge, "docs", sources.path());
    let total = chunk_count(fixture.conn());
    let embedder = EditingEmbedder {
        inner: MockEmbedder::new(),
        db_path: fixture.db_path.clone(),
        edited: AtomicBool::new(false),
    };
    fixture.app.knowledge.set_embedder(Arc::new(embedder));

    let result = fixture.app.backfill_embeddings(None).unwrap();

    assert_eq!(result, report(None, total, 1, 0));
    assert_eq!(embedding_count(fixture.conn()), total);
}

#[test]
fn failed_batch_write_rolls_back_earlier_rows_and_leaves_the_store_usable() {
    let mut fixture = Fixture::new(2);
    let sources = tempdir().unwrap();
    write_files(sources.path(), 3);
    add_synced_source(&fixture.app.knowledge, "docs", sources.path());
    let total = chunk_count(fixture.conn());
    fixture
        .app
        .knowledge
        .set_embedder(Arc::new(MisshapenEmbedder {
            inner: MockEmbedder::new(),
            bad_index: 1,
        }));

    let error = fixture.app.backfill_embeddings(None).unwrap_err();

    assert!(
        error.to_string().contains("dimension mismatch"),
        "{error:#}"
    );
    assert_eq!(
        embedding_count(fixture.conn()),
        0,
        "the first row of the failed batch must not survive the rollback"
    );

    fixture
        .app
        .knowledge
        .set_embedder(Arc::new(MockEmbedder::new()));
    let result = fixture.app.backfill_embeddings(None).unwrap();
    assert_eq!(result, report(None, total, 0, 0));
}
