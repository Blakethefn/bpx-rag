use std::path::Path;

use anyhow::{anyhow, Result};
use rusqlite::params;

use super::sources::validate_label;
use super::RagApp;

impl RagApp {
    pub fn read_chunks(
        &self,
        label: &str,
        max_chunks: usize,
        file_path: Option<&str>,
        line_start: Option<u32>,
        line_end: Option<u32>,
    ) -> Result<String> {
        validate_label(label)?;
        if line_start == Some(0) || line_end == Some(0) {
            return Err(anyhow!("line numbers must be greater than zero"));
        }
        if let (Some(start), Some(end)) = (line_start, line_end) {
            if start > end {
                return Err(anyhow!("line_start must not be greater than line_end"));
            }
        }
        if let Some(path) = file_path {
            let requested_path = Path::new(path);
            if requested_path.as_os_str().is_empty()
                || requested_path.is_absolute()
                || requested_path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(anyhow!(
                    "path must be a normalized source-relative file path"
                ));
            }
        }
        let max_chunks = max_chunks.clamp(1, 100);
        let source = self
            .knowledge
            .get_source(label)?
            .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?;
        let mut statement = self.knowledge.conn().prepare(
            "SELECT kf.rel_path, kc.title, kc.content,
                    COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0)
             FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
             JOIN knowledge_files kf ON kf.id = kcm.file_id
             WHERE kcm.source_id = ?1
               AND (?2 IS NULL OR kf.rel_path = ?2)
               AND (?3 IS NULL OR kcm.line_end >= ?3)
               AND (?4 IS NULL OR kcm.line_start <= ?4)
             ORDER BY kf.rel_path, kcm.line_start, kc.rowid
             LIMIT ?5",
        )?;
        let chunks = statement.query_map(
            params![
                source.id,
                file_path,
                line_start.map(i64::from),
                line_end.map(i64::from),
                max_chunks as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;
        let mut output = String::new();
        for chunk in chunks {
            let (path, title, content, line_start, line_end) = chunk?;
            output.push_str(&format!(
                "## {path}:{line_start}-{line_end} — {title}\n\n{content}\n\n"
            ));
        }
        if output.is_empty() {
            return Ok(format!(
                "No indexed chunks matched the requested location in source '{label}'."
            ));
        }
        Ok(output)
    }
}
