use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::embed::{self, Embed};

use super::RagApp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingBackfillReport {
    pub source_label: Option<String>,
    pub embedded_chunks: usize,
    pub skipped_chunks: usize,
    pub remaining_chunks: usize,
}

impl EmbeddingBackfillReport {
    pub fn summary(&self) -> String {
        let scope = self
            .source_label
            .as_deref()
            .map(|label| format!(" for source '{label}'"))
            .unwrap_or_else(|| " across all sources".to_string());
        format!(
            "Embedding backfill{scope}: wrote {} active-model vectors, skipped {} stale or already-completed chunks, {} remain.",
            self.embedded_chunks, self.skipped_chunks, self.remaining_chunks
        )
    }
}

type PendingRow = (i64, String, String, i64);

impl RagApp {
    pub fn backfill_embeddings(
        &mut self,
        source_label: Option<&str>,
    ) -> Result<EmbeddingBackfillReport> {
        let _writer = self.writer_lock()?;
        let source = match source_label {
            Some(label) if label.trim().is_empty() => {
                return Err(anyhow!("source label must not be empty"));
            }
            Some(label) => Some(
                self.knowledge
                    .get_source(label)?
                    .ok_or_else(|| anyhow!("knowledge source '{label}' not found"))?,
            ),
            None => None,
        };
        let source_id = source.as_ref().map(|source| source.id);
        self.ensure_embedder();
        let embedder = self.knowledge.embedder().ok_or_else(|| {
            anyhow!("embedding model is unavailable; run `bpx-rag models download` first")
        })?;
        let conn = self.knowledge.conn();
        let batch_size = self.config.embeddings.batch_size.clamp(1, 256);
        let mut report = EmbeddingBackfillReport {
            source_label: source.map(|source| source.label),
            embedded_chunks: 0,
            skipped_chunks: 0,
            remaining_chunks: 0,
        };
        loop {
            let batch = pending_batch(conn, embedder.model_id(), source_id, batch_size)?;
            if batch.is_empty() {
                break;
            }
            let texts = batch
                .iter()
                .map(|(_, title, content, _)| format!("{title} {content}"))
                .collect::<Vec<_>>();
            let text_refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
            let vectors = embedder.embed_batch(&text_refs)?;
            if vectors.len() != batch.len() {
                return Err(anyhow!(
                    "embedding provider returned an unexpected batch size"
                ));
            }
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let (written, skipped) = match write_batch(conn, embedder.as_ref(), &batch, &vectors) {
                Ok(counts) => {
                    conn.execute_batch("COMMIT")?;
                    counts
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            };
            report.embedded_chunks += written;
            report.skipped_chunks += skipped;
        }
        let remaining = count_remaining(conn, embedder.model_id(), source_id)?;
        report.remaining_chunks = remaining as usize;
        Ok(report)
    }
}

/// Chunks in scope that have no embedding for `model_id`, or one from another model.
fn pending_batch(
    conn: &Connection,
    model_id: &str,
    source_id: Option<i64>,
    batch_size: usize,
) -> Result<Vec<PendingRow>> {
    let row_mapper = |row: &rusqlite::Row<'_>| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    };
    match source_id {
        Some(source_id) => {
            let mut statement = conn.prepare(
                "SELECT kc.rowid, kc.title, kc.content, m.source_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
                   AND m.source_id = ?2
                 ORDER BY kc.rowid
                 LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![model_id, source_id, batch_size as i64], row_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }
        None => {
            let mut statement = conn.prepare(
                "SELECT kc.rowid, kc.title, kc.content, m.source_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1
                 ORDER BY kc.rowid
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![model_id, batch_size as i64], row_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }
    }
}

/// Writes the vectors whose rows still match what was selected. The caller owns the transaction
/// and rolls back on any error. Returns `(written, skipped)`.
fn write_batch(
    conn: &Connection,
    embedder: &dyn Embed,
    batch: &[PendingRow],
    vectors: &[Vec<f32>],
) -> Result<(usize, usize)> {
    let mut written = 0usize;
    let mut skipped = 0usize;
    for ((rowid, expected_title, expected_content, expected_source_id), vector) in
        batch.iter().zip(vectors.iter())
    {
        let current: Option<(String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT kc.title, kc.content, m.source_id, e.model_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE kc.rowid = ?1",
                params![rowid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((title, content, current_source_id, current_model_id)) = current else {
            skipped += 1;
            continue;
        };
        if title != *expected_title
            || content != *expected_content
            || current_source_id != *expected_source_id
            || current_model_id.as_deref() == Some(embedder.model_id())
        {
            skipped += 1;
            continue;
        }

        let vector = embed::normalize_embedding(vector, embedder.dim())?;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                *rowid,
                embed::embedding_to_bytes(&vector),
                embedder.dim() as i64,
                embedder.model_id()
            ],
        )?;
        written += 1;
    }
    Ok((written, skipped))
}

/// Chunks in scope still lacking an embedding for `model_id`.
fn count_remaining(conn: &Connection, model_id: &str, source_id: Option<i64>) -> Result<i64> {
    let count = match source_id {
        Some(source_id) => conn.query_row(
            "SELECT COUNT(*)
             FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
             WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
               AND m.source_id = ?2",
            params![model_id, source_id],
            |row| row.get::<_, i64>(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*)
             FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
             LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
             WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1",
            params![model_id],
            |row| row.get::<_, i64>(0),
        )?,
    };
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::embed::tests::MockEmbedder;
    use crate::embed::Embed;
    use crate::knowledge::KnowledgeStore;
    use crate::support::add_synced_source;
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::{tempdir, TempDir};

    /// A `RagApp` over a file-backed store. The writer lock sits beside this store's database,
    /// so the fixture needs no environment changes.
    struct Fixture {
        app: RagApp,
        db_path: PathBuf,
        _data_home: TempDir,
    }

    impl Fixture {
        fn new(batch_size: usize) -> Self {
            let data_home = tempdir().unwrap();
            let db_path = data_home.path().join("test-knowledge.db");
            let mut config = Config::default();
            config.embeddings.batch_size = batch_size;
            let app = RagApp {
                config,
                knowledge: KnowledgeStore::open_at(&db_path).unwrap(),
                embedding_notice: None,
                embedder_loaded: true,
            };
            Self {
                app,
                db_path,
                _data_home: data_home,
            }
        }

        fn conn(&self) -> &Connection {
            self.app.knowledge.conn()
        }
    }

    /// Returns the normal vectors, but one of them has the wrong length.
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

    /// Edits one chunk through a second connection during the first embedding call. That call
    /// happens after the batch is selected and before its write transaction starts.
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

    fn write_files(dir: &Path, count: usize) {
        for index in 0..count {
            fs::write(
                dir.join(format!("file{index}.rs")),
                format!("pub fn item{index}() -> u8 {{ {index} }}\n"),
            )
            .unwrap();
        }
    }

    fn count_rows(conn: &Connection, sql: &str, source_id: Option<i64>) -> usize {
        let count: i64 = match source_id {
            Some(source_id) => conn.query_row(sql, params![source_id], |row| row.get(0)),
            None => conn.query_row(sql, [], |row| row.get(0)),
        }
        .unwrap();
        count as usize
    }

    fn chunk_count(conn: &Connection) -> usize {
        count_rows(conn, "SELECT COUNT(*) FROM knowledge_chunks", None)
    }

    fn embedding_count(conn: &Connection) -> usize {
        count_rows(conn, "SELECT COUNT(*) FROM knowledge_embeddings", None)
    }

    fn source_chunk_count(conn: &Connection, source_id: i64) -> usize {
        count_rows(
            conn,
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            Some(source_id),
        )
    }

    fn source_embedding_count(conn: &Connection, source_id: i64) -> usize {
        count_rows(
            conn,
            "SELECT COUNT(*) FROM knowledge_embeddings e
             JOIN knowledge_chunk_meta m ON m.chunk_rowid = e.chunk_rowid
             WHERE m.source_id = ?1",
            Some(source_id),
        )
    }

    fn report(
        source_label: Option<&str>,
        embedded: usize,
        skipped: usize,
        remaining: usize,
    ) -> EmbeddingBackfillReport {
        EmbeddingBackfillReport {
            source_label: source_label.map(str::to_string),
            embedded_chunks: embedded,
            skipped_chunks: skipped,
            remaining_chunks: remaining,
        }
    }

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
}
