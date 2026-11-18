use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};

use super::RagApp;

mod batch;

use batch::{pending_batch, write_batch};

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
        if report.embedded_chunks > 0 {
            conn.execute_batch("PRAGMA optimize;")?;
        }
        let remaining = count_remaining(conn, embedder.model_id(), source_id)?;
        report.remaining_chunks = remaining as usize;
        Ok(report)
    }
}

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
mod tests;
