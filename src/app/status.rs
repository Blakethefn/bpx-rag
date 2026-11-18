use anyhow::Result;
use std::collections::HashMap;

use crate::embed;
use crate::knowledge::{self};
use crate::watch::{self, Holder};

use super::RagApp;

const STATUS_COUNTS: &str = "SELECT s.id, COALESCE(st.chunk_count, s.chunk_count),
    COALESCE(SUM(c.embedded_count), 0),
    MAX(COALESCE(st.embedded_count, 0) - COALESCE(SUM(c.embedded_count), 0), 0)
 FROM knowledge_sources s
 LEFT JOIN source_stats st ON st.source_id = s.id
 LEFT JOIN knowledge_files f ON f.source_id = s.id
 LEFT JOIN knowledge_embedding_coverage c INDEXED BY idx_embedding_coverage_status
   ON c.file_id = f.id AND c.model_id = ?1
 GROUP BY s.id, st.chunk_count, st.embedded_count";

#[cfg(test)]
mod tests;

impl RagApp {
    pub fn watch_status(&self) -> Result<String> {
        let db_path = knowledge::schema::knowledge_db_path()?;
        let holder = watch::holder(&db_path)?;
        let mut warnings = match &holder {
            Holder::Disabled { warning } => vec![warning.clone()],
            _ => Vec::new(),
        };
        let status = match watch::read_status(&db_path) {
            Ok(status) => status,
            Err(error) => {
                if let Some(warning) = watch::permission_warning(&error) {
                    if warnings.is_empty() {
                        warnings.push(warning);
                    }
                    None
                } else {
                    return Err(error);
                }
            }
        };
        let watched: Vec<String> = self
            .knowledge
            .watched_sources()?
            .into_iter()
            .map(|source| source.label)
            .collect();
        let mut lines = vec![match &holder {
            Holder::Running { pid } => format!(
                "watcher: running (pid {})",
                pid.as_ref()
                    .map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
            ),
            Holder::NotRunning => "watcher: not running".to_string(),
            Holder::Disabled { .. } => "watcher: disabled".to_string(),
        }];
        let running = matches!(&holder, Holder::Running { .. });
        lines.extend(
            warnings
                .into_iter()
                .map(|warning| format!("Warning: {warning}")),
        );
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
        let model_id = embed::canonical_model_id(&self.config.embeddings.model)?;
        let counts = {
            let mut statement = self.knowledge.conn().prepare(STATUS_COUNTS)?;
            let rows = statement.query_map([model_id.as_str()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    (
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ),
                ))
            })?;
            rows.collect::<rusqlite::Result<HashMap<_, _>>>()?
        };
        for source in sources {
            let (total, embedded, stale) = counts.get(&source.id).copied().unwrap_or_default();
            output.push_str(&format!(
                "- **{}** — {} files, {} chunks, {}/{} embedded; added {}; last sync: {}\n  Path: `{}`\n",
                source.label,
                source.file_count,
                total,
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
