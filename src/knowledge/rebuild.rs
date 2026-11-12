use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::knowledge::analysis::ANALYSIS_VERSION;

pub(crate) fn rebuild_source(conn: &Connection, source_id: i64, structural: bool) -> Result<()> {
    if !structural {
        let stored_version: Option<i64> = conn
            .query_row(
                "SELECT analysis_version FROM knowledge_analysis WHERE source_id = ?1",
                params![source_id],
                |row| row.get(0),
            )
            .optional()?;
        let analysis_stale = stored_version
            .map(|version| version < ANALYSIS_VERSION)
            .unwrap_or(true);
        if !analysis_stale {
            return Ok(());
        }
    }
    crate::knowledge::entries::relink_entries(conn, source_id)?;
    crate::knowledge::resolution::rebuild(conn, source_id)?;
    crate::knowledge::analysis::rebuild_analysis(conn, source_id)
}
