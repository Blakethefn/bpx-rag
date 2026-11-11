use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};

use crate::embed;

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
            let batch = {
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
                            .query_map(
                                params![embedder.model_id(), source_id, batch_size as i64],
                                row_mapper,
                            )?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        rows
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
                            .query_map(params![embedder.model_id(), batch_size as i64], row_mapper)?
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        rows
                    }
                }
            };
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
            let transaction_result = (|| -> Result<(usize, usize)> {
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
                    let Some((title, content, current_source_id, current_model_id)) = current
                    else {
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
            })();
            match transaction_result {
                Ok((written, skipped)) => {
                    conn.execute_batch("COMMIT")?;
                    report.embedded_chunks += written;
                    report.skipped_chunks += skipped;
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error);
                }
            }
        }
        let remaining = match source_id {
            Some(source_id) => conn.query_row(
                "SELECT COUNT(*)
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
                   AND m.source_id = ?2",
                params![embedder.model_id(), source_id],
                |row| row.get::<_, i64>(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*)
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1",
                params![embedder.model_id()],
                |row| row.get::<_, i64>(0),
            )?,
        };
        report.remaining_chunks = remaining as usize;
        Ok(report)
    }
}
