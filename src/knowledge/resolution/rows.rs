use anyhow::Result;
use rusqlite::{params, Connection};

use super::rust::Outcome;
use super::{status_of, StoredFile, StoredReference};

pub(super) fn write_outcome(
    insert: &mut rusqlite::CachedStatement<'_>,
    source_id: i64,
    reference_id: i64,
    outcome: &Outcome,
) -> Result<()> {
    let status = status_of(outcome);
    match outcome {
        Outcome::Unresolved => {
            insert.execute(params![
                source_id,
                reference_id,
                status,
                None::<i64>,
                None::<i64>
            ])?;
        }
        Outcome::Resolved(symbol_id) => {
            insert.execute(params![
                source_id,
                reference_id,
                status,
                symbol_id,
                None::<i64>
            ])?;
        }
        Outcome::Ambiguous(symbol_ids) => {
            for symbol_id in symbol_ids {
                insert.execute(params![
                    source_id,
                    reference_id,
                    status,
                    symbol_id,
                    None::<i64>
                ])?;
            }
        }
    }
    Ok(())
}

pub(super) fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<StoredFile>> {
    let mut statement =
        conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredFile {
            id: row.get(0)?,
            rel_path: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn load_references(conn: &Connection, source_id: i64) -> Result<Vec<StoredReference>> {
    let mut statement = conn.prepare(
        "SELECT kr.id, kr.file_id, kr.kind, kr.specifier, kr.qualifier, kf.rel_path
         FROM knowledge_references kr
         JOIN knowledge_files kf ON kf.id = kr.file_id
         WHERE kr.source_id = ?1
         ORDER BY kr.id",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredReference {
            id: row.get(0)?,
            file_id: row.get(1)?,
            kind: row.get(2)?,
            specifier: row.get(3)?,
            qualifier: row.get(4)?,
            from_rel: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
