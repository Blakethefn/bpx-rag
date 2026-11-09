//! One resolution per stored reference.
//!
//! Resolutions are derived from `knowledge_symbols` and `knowledge_references`. This pass does
//! not read source files. Symbol ids are deleted and reinserted by evidence replacement, so the
//! caller rewrites resolutions after that replacement and before commit.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::chunk::evidence::{KIND_MODULE, KIND_PATH};

mod names;
mod paths;

struct StoredFile {
    id: i64,
    rel_path: String,
}

struct StoredSymbol {
    id: i64,
    name: String,
    container: Option<String>,
    rel_path: String,
}

struct StoredReference {
    id: i64,
    file_id: i64,
    kind: String,
    specifier: String,
    qualifier: Option<String>,
    from_rel: String,
}

pub(crate) fn rebuild(conn: &Connection, source_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_resolutions WHERE source_id = ?1",
        params![source_id],
    )?;

    let files = load_files(conn, source_id)?;
    let symbols = load_symbols(conn, source_id)?;
    let references = load_references(conn, source_id)?;
    let files_by_path: HashMap<&str, i64> = files
        .iter()
        .map(|file| (file.rel_path.as_str(), file.id))
        .collect();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_resolutions
            (source_id, reference_id, status, target_symbol_id, target_file_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for reference in &references {
        if reference.kind == KIND_MODULE || reference.kind == KIND_PATH {
            match paths::module_file(
                &reference.from_rel,
                &reference.specifier,
                &files_by_path,
                reference.file_id,
            ) {
                Some(file_id) => {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "resolved",
                        None::<i64>,
                        file_id
                    ])?;
                }
                None => {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "unresolved",
                        None::<i64>,
                        None::<i64>
                    ])?;
                }
            }
            continue;
        }

        let candidates = names::symbol_candidates(reference, &symbols);
        match candidates.as_slice() {
            [] => {
                insert.execute(params![
                    source_id,
                    reference.id,
                    "unresolved",
                    None::<i64>,
                    None::<i64>
                ])?;
            }
            [symbol_id] => {
                insert.execute(params![
                    source_id,
                    reference.id,
                    "resolved",
                    symbol_id,
                    None::<i64>
                ])?;
            }
            many => {
                for symbol_id in many {
                    insert.execute(params![
                        source_id,
                        reference.id,
                        "ambiguous",
                        symbol_id,
                        None::<i64>
                    ])?;
                }
            }
        }
    }
    Ok(())
}

fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<StoredFile>> {
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

fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<StoredSymbol>> {
    let mut statement = conn.prepare(
        "SELECT ks.id, ks.name, ks.container, kf.rel_path
         FROM knowledge_symbols ks
         JOIN knowledge_files kf ON kf.id = ks.file_id
         WHERE ks.source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(StoredSymbol {
            id: row.get(0)?,
            name: row.get(1)?,
            container: row.get(2)?,
            rel_path: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_references(conn: &Connection, source_id: i64) -> Result<Vec<StoredReference>> {
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

#[cfg(test)]
mod tests;
