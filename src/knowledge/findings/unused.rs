//! Possibly-unused Rust symbols from a complete analysis.
//!
//! A symbol is unused when sealed-scope resolution never targets it. `pub` on a library tree,
//! or on no program tree, is withheld. Null visibility is withheld. Non-Rust files are omitted.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

use crate::chunk::evidence::STATUS_COMPLETE;
use crate::knowledge::analysis::{FileRow, ANALYSIS_VERSION};
use crate::knowledge::resolution::{Symbol, Tree};

const UNUSED_REASON: &str = "no reference in sealed scope";

pub(crate) fn write(
    conn: &Connection,
    source_id: i64,
    generation: i64,
    fingerprint: &str,
    files: &[FileRow],
    tree: &Tree,
) -> Result<()> {
    let used = used_symbols(conn, source_id)?;
    let complete_files: HashSet<i64> = files
        .iter()
        .filter(|file| file.symbol_status == STATUS_COMPLETE)
        .map(|file| file.id)
        .collect();
    let paths: HashMap<i64, &str> = files
        .iter()
        .map(|file| (file.id, file.rel_path.as_str()))
        .collect();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_findings
            (source_id, kind, file_id, symbol_id, reason, analysis_version, evidence_generation, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut unused: Vec<&Symbol> = tree
        .symbols()
        .iter()
        .filter(|symbol| reportable(tree, symbol, &used, &complete_files))
        .collect();
    unused.sort_by(|left, right| {
        let left_path = paths.get(&left.file_id).copied().unwrap_or("");
        let right_path = paths.get(&right.file_id).copied().unwrap_or("");
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

fn reportable(
    tree: &Tree,
    symbol: &Symbol,
    used: &HashSet<i64>,
    complete_files: &HashSet<i64>,
) -> bool {
    if !complete_files.contains(&symbol.file_id) || !tree.is_rust_file(symbol.file_id) {
        return false;
    }
    if symbol.kind == "mod" || used.contains(&symbol.id) {
        return false;
    }
    if tree.is_program_main(symbol) || tree.is_named_test(symbol) {
        return false;
    }
    match symbol.visibility.as_deref() {
        Some("private" | "pub(self)" | "pub(super)" | "pub(crate)") => true,
        Some(visibility) if visibility.starts_with("pub(in ") => true,
        Some("pub") => tree.on_program_only(symbol.file_id),
        _ => false,
    }
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
