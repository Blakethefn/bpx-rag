use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

use crate::chunk::evidence::STATUS_COMPLETE;

mod rows;
mod seal;

pub(crate) const ANALYSIS_VERSION: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileRow {
    pub(crate) id: i64,
    pub(crate) rel_path: String,
    pub(crate) content_hash: String,
    pub(crate) symbol_status: String,
    pub(crate) reference_status: String,
    pub(crate) reference_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryRow {
    pub(crate) target_file_id: Option<i64>,
    pub(crate) target_rel_path: String,
    pub(crate) kind: String,
    pub(crate) evidence_hash: String,
    pub(crate) evidence_file_id: i64,
}

pub(crate) fn rebuild_analysis(conn: &Connection, source_id: i64) -> Result<()> {
    let files = rows::load_files(conn, source_id)?;
    let entries = rows::load_entries(conn, source_id)?;
    let tree = crate::knowledge::resolution::Tree::load(conn, source_id)?;
    let previous: i64 = conn
        .query_row(
            "SELECT evidence_generation FROM knowledge_analysis WHERE source_id = ?1",
            params![source_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let generation = previous + 1;
    let fingerprint = rows::fingerprint(&files, &entries);
    let unsealed = seal::unsealed_rust_files(&files, &tree);
    let gaps = gaps(&files, &entries, &tree, &unsealed);
    let status = seal::analysis_status(&files, &unsealed);

    conn.execute(
        "DELETE FROM knowledge_findings WHERE source_id = ?1",
        params![source_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_analysis WHERE source_id = ?1",
        params![source_id],
    )?;
    conn.execute(
        "INSERT INTO knowledge_analysis
            (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            source_id,
            ANALYSIS_VERSION,
            generation,
            fingerprint,
            status,
            gaps
        ],
    )?;
    if status != "incomplete" {
        crate::knowledge::findings::unused::write(
            conn,
            source_id,
            generation,
            &fingerprint,
            &files,
            &tree,
            &unsealed,
        )?;
        crate::knowledge::findings::reachability::write(
            conn,
            source_id,
            generation,
            &fingerprint,
            &files,
            &tree,
            &unsealed,
        )?;
    }
    Ok(())
}

fn gaps(
    files: &[FileRow],
    entries: &[EntryRow],
    tree: &crate::knowledge::resolution::Tree,
    unsealed: &HashSet<i64>,
) -> String {
    let mut lines = Vec::new();
    for file in files {
        if file.symbol_status != STATUS_COMPLETE {
            lines.push(format!("{}:{}", file.rel_path, file.symbol_status));
        } else if file.reference_status != STATUS_COMPLETE {
            lines.push(format!("{}:{}", file.rel_path, file.reference_status));
        } else if !crate::knowledge::resolution::is_rust_path(&file.rel_path) {
            lines.push(format!("{}:resolution_unsupported", file.rel_path));
        }
        if file.reference_version < crate::knowledge::references::REFERENCE_INDEX_VERSION {
            lines.push(format!("{}:reference_version", file.rel_path));
        }
        if unsealed.contains(&file.id) || tree.unresolved_mod_candidates().contains(&file.id) {
            lines.push(format!("{}:unsealed", file.rel_path));
        }
    }
    if tree.membership_roots().is_empty() {
        lines.push("no_entry_points".to_string());
    }
    lines.extend(tree.limitation_lines());
    for entry in entries {
        if entry.target_file_id.is_none() {
            lines.push(format!("{}:missing", entry.target_rel_path));
        }
    }
    lines.sort();
    lines.dedup();
    lines.join("\n")
}

#[cfg(test)]
mod tests;
