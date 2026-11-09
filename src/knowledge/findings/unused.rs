//! Possibly-unused symbols from a complete analysis.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashSet;

use crate::chunk::evidence::STATUS_COMPLETE;
use crate::knowledge::analysis::{FileRow, ANALYSIS_VERSION};

const UNUSED_REASON: &str = "no indexed reference";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolRow {
    pub(crate) id: i64,
    pub(crate) file_id: i64,
    pub(crate) name: String,
}

pub(crate) fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<SymbolRow>> {
    let mut statement =
        conn.prepare("SELECT id, file_id, name FROM knowledge_symbols WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(SymbolRow {
            id: row.get(0)?,
            file_id: row.get(1)?,
            name: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn write(
    conn: &Connection,
    source_id: i64,
    generation: i64,
    fingerprint: &str,
    files: &[FileRow],
    symbols: &[SymbolRow],
) -> Result<()> {
    let used = used_symbols(conn, source_id)?;
    let complete_files: HashSet<i64> = files
        .iter()
        .filter(|file| file.symbol_status == STATUS_COMPLETE)
        .map(|file| file.id)
        .collect();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_findings
            (source_id, kind, file_id, symbol_id, reason, analysis_version, evidence_generation, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut unused: Vec<&SymbolRow> = symbols
        .iter()
        .filter(|symbol| complete_files.contains(&symbol.file_id) && !used.contains(&symbol.id))
        .collect();
    unused.sort_by(|left, right| {
        let left_path = files
            .iter()
            .find(|file| file.id == left.file_id)
            .map(|file| file.rel_path.as_str())
            .unwrap_or("");
        let right_path = files
            .iter()
            .find(|file| file.id == right.file_id)
            .map(|file| file.rel_path.as_str())
            .unwrap_or("");
        (left_path, left.name.as_str(), left.id).cmp(&(right_path, right.name.as_str(), right.id))
    });
    for symbol in unused {
        insert.execute(params![
            source_id,
            "possibly_unused_symbol",
            symbol.file_id,
            symbol.id,
            UNUSED_REASON,
            ANALYSIS_VERSION,
            generation,
            fingerprint
        ])?;
    }
    Ok(())
}

fn used_symbols(conn: &Connection, source_id: i64) -> Result<HashSet<i64>> {
    let mut statement = conn.prepare(
        "SELECT target_symbol_id FROM knowledge_resolutions
         WHERE source_id = ?1
           AND status IN ('resolved', 'ambiguous')
           AND target_symbol_id IS NOT NULL",
    )?;
    let rows = statement.query_map(params![source_id], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}
