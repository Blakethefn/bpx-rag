use anyhow::Result;
use rusqlite::{params, Connection};

use super::SYMBOL_INDEX_VERSION;
use crate::chunk::Symbol;

/// Replace a file's symbol rows and mark the file as current. Runs inside the sync transaction.
///
/// The file's `content_hash` must already hold the content the symbols came from. The rows
/// are stamped with that hash, so a later content change by an older writer marks them stale.
/// `status` is written in the same update. Only `complete` stores symbol rows: an empty list
/// then means the walk finished and the file declared nothing.
pub(crate) fn replace_file_symbols(
    conn: &Connection,
    source_id: i64,
    file_id: i64,
    status: &str,
    symbols: &[Symbol],
) -> Result<()> {
    delete_file_symbols(conn, file_id)?;
    if status == crate::chunk::evidence::STATUS_COMPLETE {
        let mut insert = conn.prepare_cached(
            "INSERT INTO knowledge_symbols
                (source_id, file_id, name, kind, container, line_start, line_end, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for symbol in symbols {
            insert.execute(params![
                source_id,
                file_id,
                symbol.name,
                symbol.kind,
                symbol.container,
                symbol.line_start,
                symbol.line_end,
                symbol.visibility,
            ])?;
        }
    }
    conn.execute(
        "UPDATE knowledge_files
         SET symbol_version = ?1,
             symbol_status = ?2,
             symbols_hash = (SELECT content_hash FROM knowledge_files WHERE id = ?3)
         WHERE id = ?3",
        params![SYMBOL_INDEX_VERSION, status, file_id],
    )?;
    Ok(())
}

/// Remove one file's symbol rows. Called wherever the file's chunks are removed.
pub(crate) fn delete_file_symbols(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_symbols WHERE file_id = ?1",
        params![file_id],
    )?;
    Ok(())
}
