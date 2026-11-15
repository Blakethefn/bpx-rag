use anyhow::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use super::{EntryRow, FileRow, ANALYSIS_VERSION};

pub(super) fn fingerprint(files: &[FileRow], entries: &[EntryRow]) -> String {
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

pub(super) fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<FileRow>> {
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

pub(super) fn load_entries(conn: &Connection, source_id: i64) -> Result<Vec<EntryRow>> {
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
