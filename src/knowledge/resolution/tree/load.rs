use anyhow::Result;
use rusqlite::{params, Connection};

use super::{ModRef, Symbol};

pub(super) fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<(i64, String)>> {
    let mut statement =
        conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<Symbol>> {
    let mut statement = conn.prepare(
        "SELECT id, file_id, name, kind, container, visibility
         FROM knowledge_symbols WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(Symbol {
            id: row.get(0)?,
            file_id: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
            container: row.get(4)?,
            visibility: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn load_modules(conn: &Connection, source_id: i64) -> Result<Vec<ModRef>> {
    let mut statement = conn.prepare(
        "SELECT kr.file_id, kf.rel_path, kr.specifier, kr.qualifier
         FROM knowledge_references kr
         JOIN knowledge_files kf ON kf.id = kr.file_id
         WHERE kr.source_id = ?1 AND kr.kind = 'module'",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(ModRef {
            file_id: row.get(0)?,
            from_rel: row.get(1)?,
            name: row.get(2)?,
            qualifier: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn load_entries(
    conn: &Connection,
    source_id: i64,
) -> Result<Vec<(Option<i64>, String, String)>> {
    let mut statement = conn.prepare(
        "SELECT target_file_id, kind, reason FROM knowledge_entries WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
