use super::*;
use crate::knowledge::KnowledgeStore;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Default)]
pub struct SensitiveIndexScrubReport {
    pub files_removed: usize,
    pub chunks_removed: usize,
}

impl SensitiveIndexScrubReport {
    pub fn summary(&self) -> String {
        format!(
            "Sensitive index scrub: removed {} files and {} chunks.",
            self.files_removed, self.chunks_removed
        )
    }
}

/// Remove already-indexed VCS metadata and files containing a recognized
/// Google API-key signature. This reads only stored index rows and never walks
/// source directories or prints paths/content.
pub fn scrub_sensitive_indexed_files(
    store: &KnowledgeStore,
    source_id: Option<i64>,
) -> Result<SensitiveIndexScrubReport> {
    let conn = store.conn();
    let mut candidates = HashMap::<i64, i64>::new();
    {
        let mut statement = conn.prepare(
            "SELECT id, source_id, rel_path FROM knowledge_files
             WHERE (?1 IS NULL OR source_id = ?1)",
        )?;
        let rows = statement.query_map(params![source_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (file_id, file_source_id, rel_path) = row?;
            if is_sensitive_relative_path(Path::new(&rel_path)) {
                candidates.insert(file_id, file_source_id);
            }
        }
    }

    {
        let mut statement = conn.prepare(
            "SELECT kf.id, kf.source_id, kc.content
             FROM knowledge_files kf
             JOIN knowledge_chunk_meta m ON m.file_id = kf.id
             JOIN knowledge_chunks kc ON kc.rowid = m.chunk_rowid
             WHERE (?1 IS NULL OR kf.source_id = ?1)
               AND instr(kc.content, 'AIza') > 0",
        )?;
        let rows = statement.query_map(params![source_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (file_id, file_source_id, content) = row?;
            if contains_google_api_key(&content) {
                candidates.insert(file_id, file_source_id);
            }
        }
    }

    if candidates.is_empty() {
        return Ok(SensitiveIndexScrubReport::default());
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let transaction_result = (|| -> Result<SensitiveIndexScrubReport> {
        let mut report = SensitiveIndexScrubReport::default();
        let mut affected_sources = HashSet::new();

        for (file_id, file_source_id) in candidates {
            let relative_path: Option<String> = conn
                .query_row(
                    "SELECT rel_path FROM knowledge_files WHERE id = ?1 AND source_id = ?2",
                    params![file_id, file_source_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(relative_path) = relative_path else {
                continue;
            };

            let mut sensitive = is_sensitive_relative_path(Path::new(&relative_path));
            if !sensitive {
                let mut statement = conn.prepare(
                    "SELECT kc.content FROM knowledge_chunk_meta m
                     JOIN knowledge_chunks kc ON kc.rowid = m.chunk_rowid
                     WHERE m.file_id = ?1",
                )?;
                let rows = statement.query_map(params![file_id], |row| row.get::<_, String>(0))?;
                for row in rows {
                    if contains_google_api_key(&row?) {
                        sensitive = true;
                        break;
                    }
                }
            }
            if !sensitive {
                continue;
            }

            let chunk_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE file_id = ?1",
                params![file_id],
                |row| row.get(0),
            )?;
            delete_file_chunks(conn, file_id)?;
            let deleted = conn.execute(
                "DELETE FROM knowledge_files WHERE id = ?1 AND source_id = ?2",
                params![file_id, file_source_id],
            )?;
            if deleted == 0 {
                continue;
            }
            report.files_removed += 1;
            report.chunks_removed += chunk_count.max(0) as usize;
            affected_sources.insert(file_source_id);
        }

        for file_source_id in affected_sources {
            conn.execute(
                "UPDATE knowledge_sources SET
                    file_count = (SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1),
                    chunk_count = (SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1)
                 WHERE id = ?1",
                params![file_source_id],
            )?;
            // A removed file changes the evidence set. Rebuild from the rows still stored.
            crate::knowledge::analysis::rebuild_source(conn, file_source_id, true)?;
        }

        Ok(report)
    })();

    match transaction_result {
        Ok(report) => {
            conn.execute_batch("COMMIT")?;
            Ok(report)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────
