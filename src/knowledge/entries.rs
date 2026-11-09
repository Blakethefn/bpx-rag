//! Stored entry rows.
//!
//! An entry names a path and may leave the file id null when that path is not indexed.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::chunk::evidence::EntryObservation;

/// Replace the observations whose evidence is this file. Target file ids are filled later,
/// after every path in the source is known.
pub(crate) fn replace_file_entries(
    conn: &Connection,
    source_id: i64,
    file_id: i64,
    rel_path: &str,
    evidence_hash: &str,
    entries: &[EntryObservation],
) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_entries WHERE evidence_file_id = ?1",
        params![file_id],
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_entries
            (source_id, evidence_file_id, target_file_id, target_rel_path, kind, reason, evidence_hash)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
    )?;
    for entry in entries {
        let target = entry
            .target_rel_path
            .clone()
            .unwrap_or_else(|| rel_path.replace('\\', "/"));
        insert.execute(params![
            source_id,
            file_id,
            target,
            entry.kind,
            entry.reason,
            evidence_hash,
        ])?;
    }
    Ok(())
}

/// Point each entry at the indexed file for its path. A missing path stays null.
pub(crate) fn relink_entries(conn: &Connection, source_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE knowledge_entries
         SET target_file_id = (
             SELECT id FROM knowledge_files
             WHERE source_id = knowledge_entries.source_id
               AND rel_path = knowledge_entries.target_rel_path
         )
         WHERE source_id = ?1",
        params![source_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
