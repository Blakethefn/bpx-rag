//! Stored reference rows.
//!
//! A reference has no target id.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::chunk::evidence::{Reference, STATUS_COMPLETE};

/// Version of the reference index. A bump backfills references and entries on the next sync.
///
/// Version 3 puts the function name in a `#[test]` entry reason so two tests in one file stay
/// distinct, and so unused-symbol analysis can see which function the attribute names.
pub(crate) const REFERENCE_INDEX_VERSION: i64 = 3;

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
