//! One resolution per stored reference.
//!
//! Resolutions are derived from `knowledge_symbols` and `knowledge_references`. This pass does
//! not read source files. Symbol ids are deleted and reinserted by evidence replacement, so the
//! caller rewrites resolutions after that replacement and before commit.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::chunk::evidence::{KIND_IMPORT, KIND_MODULE, KIND_NAME, KIND_PATH, KIND_TYPE};

mod paths;
mod rust;
mod tree;

pub(crate) use tree::is_rust_path;
pub(crate) use tree::{Symbol, Tree, ENTRY_KINDS};

const STATUS_RESOLVED: &str = "resolved";
const STATUS_AMBIGUOUS: &str = "ambiguous";
const STATUS_UNRESOLVED: &str = "unresolved";

pub(crate) const RESOLUTION_STATUSES: &[&str] =
    &[STATUS_RESOLVED, STATUS_AMBIGUOUS, STATUS_UNRESOLVED];

fn status_of(outcome: &rust::Outcome) -> &'static str {
    match outcome {
        rust::Outcome::Resolved(_) => STATUS_RESOLVED,
        rust::Outcome::Ambiguous(_) => STATUS_AMBIGUOUS,
        rust::Outcome::Unresolved => STATUS_UNRESOLVED,
    }
}

struct StoredFile {
    id: i64,
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
    let references = load_references(conn, source_id)?;
    let files_by_path: HashMap<&str, i64> = files
        .iter()
        .map(|file| (file.rel_path.as_str(), file.id))
        .collect();
    let tree = tree::Tree::load(conn, source_id)?;
    let rust_outcomes = rust::resolve(&tree, &references);

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_resolutions
            (source_id, reference_id, status, target_symbol_id, target_file_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for reference in &references {
        if reference.kind == KIND_MODULE || reference.kind == KIND_PATH {
            let file_id = paths::module_file(
                &reference.from_rel,
                &reference.specifier,
                &files_by_path,
                reference.file_id,
            );
            // The outcome only selects the status. This branch stores a file, not a symbol.
            let outcome = match file_id {
                Some(_) => rust::Outcome::Resolved(0),
                None => rust::Outcome::Unresolved,
            };
            insert.execute(params![
                source_id,
                reference.id,
                status_of(&outcome),
                None::<i64>,
                file_id
            ])?;
            continue;
        }

        // Rust names resolve in the referring file's module. Other languages have no resolver,
        // so a same-named symbol in another file is not a candidate.
        let outcome = if is_rust_name(reference) {
            rust_outcomes
                .get(&reference.id)
                .cloned()
                .unwrap_or(rust::Outcome::Unresolved)
        } else {
            rust::Outcome::Unresolved
        };
        write_outcome(&mut insert, source_id, reference.id, &outcome)?;
    }
    Ok(())
}

fn is_rust_name(reference: &StoredReference) -> bool {
    tree::is_rust_path(&reference.from_rel)
        && matches!(reference.kind.as_str(), KIND_NAME | KIND_IMPORT | KIND_TYPE)
}

fn write_outcome(
    insert: &mut rusqlite::CachedStatement<'_>,
    source_id: i64,
    reference_id: i64,
    outcome: &rust::Outcome,
) -> Result<()> {
    let status = status_of(outcome);
    match outcome {
        rust::Outcome::Unresolved => {
            insert.execute(params![
                source_id,
                reference_id,
                status,
                None::<i64>,
                None::<i64>
            ])?;
        }
        rust::Outcome::Resolved(symbol_id) => {
            insert.execute(params![
                source_id,
                reference_id,
                status,
                symbol_id,
                None::<i64>
            ])?;
        }
        rust::Outcome::Ambiguous(symbol_ids) => {
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
