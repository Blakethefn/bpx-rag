use anyhow::Result;
use rusqlite::params;

use crate::embed;
use crate::knowledge::{self};
use crate::watch::{self, Holder};

use super::RagApp;

impl RagApp {
    /// The watcher lease holder and each source's watch setting and last reported state.
    pub fn watch_status(&self) -> Result<String> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        let holder = watch::holder(&db_path)?;
        let status = watch::read_status(&db_path)?;
        let watched: Vec<String> = self
            .knowledge
            .watched_sources()?
            .into_iter()
            .map(|source| source.label)
            .collect();
        let mut lines = vec![match holder {
            Holder::Running { pid } => format!(
                "watcher: running (pid {})",
                pid.map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
            ),
            Holder::NotRunning => "watcher: not running".to_string(),
        }];
        let running = matches!(holder, Holder::Running { .. });
        for source in self.knowledge.list_sources()? {
            if !watched.contains(&source.label) {
                lines.push(format!("{}: watch disabled", source.label));
                continue;
            }
            let reported = status.as_ref().and_then(|status| {
                status
                    .sources
                    .iter()
                    .find(|entry| entry.label == source.label)
            });
            let detail = match reported {
                Some(entry) => format!(
                    "state {}, batches {}, reindexed {}, reconciled at {}, last error {}{}{}",
                    serde_json::to_value(entry.state)?
                        .as_str()
                        .unwrap_or("unknown"),
                    entry.batches,
                    entry.reindexed_files,
                    entry
                        .reconciled_at
                        .map_or_else(|| "never".to_string(), |at| at.to_string()),
                    entry.last_error.as_deref().unwrap_or("none"),
                    if running {
                        ""
                    } else {
                        " (no watcher is running)"
                    },
                    skipped_note(&entry.skipped_files),
                ),
                None => "no watcher has reported yet".to_string(),
            };
            lines.push(format!("{}: watch enabled; {detail}", source.label));
        }
        Ok(lines.join("\n"))
    }

    pub fn source_status(&self) -> Result<String> {
        let sources = self.knowledge.list_sources()?;
        if sources.is_empty() {
            return Ok("No sources are registered. Add one with `bpx-rag add`.".into());
        }
        let mut output = String::from("# Knowledge sources\n\n");
        for source in sources {
            let model_id = embed::canonical_model_id(&self.config.embeddings.model)?;
            let (embedded, stale, total): (i64, i64, i64) = self.knowledge.conn().query_row(
                "SELECT COUNT(DISTINCT CASE WHEN e.model_id = ?1 THEN m.chunk_rowid END),
                        COUNT(DISTINCT CASE WHEN e.model_id IS NOT NULL AND e.model_id != ?1 THEN m.chunk_rowid END),
                        COUNT(DISTINCT m.chunk_rowid)
                 FROM knowledge_chunk_meta m
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = m.chunk_rowid
                 WHERE m.source_id = ?2",
                params![model_id, source.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            output.push_str(&format!(
                "- **{}** — {} files, {} chunks, {}/{} embedded; added {}; last sync: {}\n  Path: `{}`\n",
                source.label,
                source.file_count,
                source.chunk_count,
                embedded,
                total,
                source.created_at,
                source.last_sync.as_deref().unwrap_or("never"),
                source.path
            ));
            if stale > 0 {
                output.push_str(&format!(
                    "  Warning: {stale} chunks have vectors from another model and are not searchable semantically; run `bpx-rag embed --source {}`\n",
                    source.label
                ));
            }
        }
        output.push_str(&format!(
            "\nEmbedding model: `{}` ({})\n",
            self.config.embeddings.model,
            self.embedding_model_status()
        ));
        Ok(output)
    }
}

/// One status line for files a watcher sync skipped, with each file's reason.
fn skipped_note(skipped: &[knowledge::sync::SkippedFile]) -> String {
    if skipped.is_empty() {
        return String::new();
    }
    let names: Vec<String> = skipped
        .iter()
        .map(|file| format!("{} ({})", file.rel_path, file.reason))
        .collect();
    format!(", skipped {} file(s): {}", skipped.len(), names.join(", "))
}
