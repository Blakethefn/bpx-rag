//! Derived analysis for one source.
//!
//! `knowledge_analysis` is one row per source. Findings are written only when that row is
//! complete.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::chunk::evidence::{STATUS_COMPLETE, STATUS_UNSUPPORTED};
use crate::knowledge::references::REFERENCE_INDEX_VERSION;

pub(crate) const ANALYSIS_VERSION: i64 = 2;

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

/// Replace the analysis row. Finding rows are written only when that row is complete.
pub(crate) fn rebuild_analysis(conn: &Connection, source_id: i64) -> Result<()> {
    let files = load_files(conn, source_id)?;
    let entries = load_entries(conn, source_id)?;
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
    let fingerprint = fingerprint(&files, &entries);
    let gaps = gaps(&files, &entries, &tree);
    let status = if incomplete(&files) {
        "incomplete"
    } else {
        "complete"
    };

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
    if status == "complete" {
        crate::knowledge::findings::unused::write(
            conn,
            source_id,
            generation,
            &fingerprint,
            &files,
            &tree,
        )?;
        crate::knowledge::findings::reachability::write(
            conn,
            source_id,
            generation,
            &fingerprint,
            &files,
            &entries,
        )?;
    }
    Ok(())
}

fn incomplete(files: &[FileRow]) -> bool {
    files.iter().any(|file| {
        file.reference_version < REFERENCE_INDEX_VERSION
            || blocks_conclusion(&file.symbol_status)
            || blocks_conclusion(&file.reference_status)
    })
}

fn blocks_conclusion(status: &str) -> bool {
    status != STATUS_COMPLETE && status != STATUS_UNSUPPORTED
}

fn gaps(
    files: &[FileRow],
    entries: &[EntryRow],
    tree: &crate::knowledge::resolution::Tree,
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
        if file.reference_version < REFERENCE_INDEX_VERSION {
            lines.push(format!("{}:reference_version", file.rel_path));
        }
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

fn fingerprint(files: &[FileRow], entries: &[EntryRow]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("analysis_version:{ANALYSIS_VERSION}\n"));
    let mut files: Vec<&FileRow> = files.iter().collect();
    files.sort_by_key(|file| file.id);
    for file in files {
        hasher.update(format!(
            "file:{}:{}:{}:{}\n",
            file.id, file.content_hash, file.symbol_status, file.reference_status
        ));
    }
    let mut entries: Vec<&EntryRow> = entries.iter().collect();
    entries.sort_by(|left, right| {
        (
            &left.evidence_hash,
            &left.kind,
            &left.target_rel_path,
            left.evidence_file_id,
        )
            .cmp(&(
                &right.evidence_hash,
                &right.kind,
                &right.target_rel_path,
                right.evidence_file_id,
            ))
    });
    for entry in entries {
        hasher.update(format!(
            "entry:{}:{}:{}\n",
            entry.evidence_hash, entry.kind, entry.target_rel_path
        ));
    }
    format!("{:x}", hasher.finalize())
}

fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<FileRow>> {
    let mut statement = conn.prepare(
        "SELECT id, rel_path, content_hash,
                COALESCE(symbol_status, ''),
                COALESCE(reference_status, ''),
                reference_version
         FROM knowledge_files WHERE source_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(FileRow {
            id: row.get(0)?,
            rel_path: row.get(1)?,
            content_hash: row.get(2)?,
            symbol_status: row.get(3)?,
            reference_status: row.get(4)?,
            reference_version: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_entries(conn: &Connection, source_id: i64) -> Result<Vec<EntryRow>> {
    let mut statement = conn.prepare(
        "SELECT target_file_id, target_rel_path, kind, evidence_hash, evidence_file_id
         FROM knowledge_entries WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(EntryRow {
            target_file_id: row.get(0)?,
            target_rel_path: row.get(1)?,
            kind: row.get(2)?,
            evidence_hash: row.get(3)?,
            evidence_file_id: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests;
