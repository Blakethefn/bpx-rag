//! Stored reference and entry rows.
//!
//! These are evidence. A reference has no target id. An entry names a path and may leave the
//! file id null when that path is not indexed.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::chunk::evidence::{EntryObservation, Reference, STATUS_COMPLETE};
use crate::chunk::Symbol;

/// Version of the reference index. A bump backfills references on the next sync.
pub(crate) const REFERENCE_INDEX_VERSION: i64 = 1;

/// Replace one file's reference rows and stamp the file in the same way as symbols.
///
/// A parse error or a file over the tree-sitter cap stores no reference rows. The hash still
/// records which bytes were considered.
pub(crate) fn replace_file_references(
    conn: &Connection,
    source_id: i64,
    file_id: i64,
    status: &str,
    references: &[Reference],
) -> Result<()> {
    delete_file_references(conn, file_id)?;
    // Only a finished walk stores occurrences. Unsupported, parse_error, and too_large files
    // keep the stamp and no rows, so a previous file's occurrences cannot survive.
    if status == STATUS_COMPLETE {
        let mut insert = conn.prepare_cached(
            "INSERT INTO knowledge_references
                (source_id, file_id, line_start, line_end, kind, specifier, qualifier)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for reference in references {
            insert.execute(params![
                source_id,
                file_id,
                reference.line_start,
                reference.line_end,
                reference.kind,
                reference.specifier,
                reference.qualifier,
            ])?;
        }
    }
    conn.execute(
        "UPDATE knowledge_files
         SET reference_version = ?1,
             reference_status = ?2,
             references_hash = (SELECT content_hash FROM knowledge_files WHERE id = ?3)
         WHERE id = ?3",
        params![REFERENCE_INDEX_VERSION, status, file_id],
    )?;
    Ok(())
}

pub(crate) fn delete_file_references(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_references WHERE file_id = ?1",
        params![file_id],
    )?;
    Ok(())
}

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

/// Write symbols, references, and entries for one file, then stamp both hashes from the
/// content hash already stored on the file row.
pub(crate) fn replace_file_evidence(
    conn: &Connection,
    source_id: i64,
    file_id: i64,
    rel_path: &str,
    status: &str,
    symbols: &[Symbol],
    references: &[Reference],
    entries: &[EntryObservation],
) -> Result<()> {
    let evidence_hash: String = conn.query_row(
        "SELECT content_hash FROM knowledge_files WHERE id = ?1",
        params![file_id],
        |row| row.get(0),
    )?;
    crate::knowledge::symbols::replace_file_symbols(conn, source_id, file_id, status, symbols)?;
    replace_file_references(conn, source_id, file_id, status, references)?;
    replace_file_entries(conn, source_id, file_id, rel_path, &evidence_hash, entries)?;
    Ok(())
}
