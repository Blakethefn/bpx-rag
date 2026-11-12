use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::chunk::evidence::EntryObservation;

const IMPLICIT_REASONS: &[&str] = &[
    "Cargo.toml implicit lib",
    "Cargo.toml implicit bin",
    "Cargo.toml implicit example",
    "Cargo.toml implicit test",
    "Cargo.toml implicit bench",
];

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

pub(crate) fn refresh_implicit(
    conn: &Connection,
    source_id: i64,
    source_root: &Path,
) -> Result<()> {
    let mut files = conn
        .prepare("SELECT id, rel_path, content_hash FROM knowledge_files WHERE source_id = ?1")?;
    let rows = files.query_map(params![source_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let stored: Vec<(i64, String, String)> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(files);
    let indexed: HashSet<String> = stored
        .iter()
        .map(|(_, rel_path, _)| rel_path.replace('\\', "/"))
        .collect();
    for (file_id, rel_path, evidence_hash) in &stored {
        let rel_path = rel_path.replace('\\', "/");
        if rel_path.rsplit('/').next().unwrap_or(&rel_path) != "Cargo.toml" {
            continue;
        }
        let abs = source_root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let content = fs::read_to_string(&abs)
            .with_context(|| format!("failed to read manifest {}", abs.display()))?;
        let implicit =
            crate::chunk::evidence::implicit_cargo_entries(&rel_path, &content, &indexed);
        conn.execute(
            "DELETE FROM knowledge_entries
             WHERE evidence_file_id = ?1
               AND reason IN (?2, ?3, ?4, ?5, ?6)",
            params![
                file_id,
                IMPLICIT_REASONS[0],
                IMPLICIT_REASONS[1],
                IMPLICIT_REASONS[2],
                IMPLICIT_REASONS[3],
                IMPLICIT_REASONS[4],
            ],
        )?;
        let mut insert = conn.prepare_cached(
            "INSERT INTO knowledge_entries
                (source_id, evidence_file_id, target_file_id, target_rel_path, kind, reason, evidence_hash)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
        )?;
        for entry in implicit {
            let Some(target) = entry.target_rel_path else {
                continue;
            };
            insert.execute(params![
                source_id,
                file_id,
                target,
                entry.kind,
                entry.reason,
                evidence_hash,
            ])?;
        }
    }
    Ok(())
}

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
