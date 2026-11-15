use anyhow::Result;
use rusqlite::{params, Connection};

use super::{PendingSymbols, SYMBOL_INDEX_VERSION};

pub(super) fn pending_sources(conn: &Connection, source_id: Option<i64>) -> Result<Vec<PendingSymbols>> {
    let mut statement = conn.prepare(
        "SELECT s.label, COUNT(*)
         FROM knowledge_files kf
         JOIN knowledge_sources s ON s.id = kf.source_id
         WHERE (kf.symbol_version < ?1 OR kf.symbols_hash IS NOT kf.content_hash)
           AND (?2 IS NULL OR kf.source_id = ?2)
         GROUP BY s.label
         ORDER BY s.label",
    )?;
    let rows = statement.query_map(params![SYMBOL_INDEX_VERSION, source_id], |row| {
        Ok(PendingSymbols {
            source_label: row.get(0)?,
            files: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
