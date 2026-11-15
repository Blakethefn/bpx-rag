use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::chunk::evidence::{KIND_IMPORT, KIND_MODULE, KIND_NAME, KIND_PATH, KIND_TYPE};

mod paths;
mod rows;
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

    let files = rows::load_files(conn, source_id)?;
    let references = rows::load_references(conn, source_id)?;
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

        let outcome = if is_rust_name(reference) {
            rust_outcomes
                .get(&reference.id)
                .cloned()
                .unwrap_or(rust::Outcome::Unresolved)
        } else {
            rust::Outcome::Unresolved
        };
        rows::write_outcome(&mut insert, source_id, reference.id, &outcome)?;
    }
    Ok(())
}

fn is_rust_name(reference: &StoredReference) -> bool {
    tree::is_rust_path(&reference.from_rel)
        && matches!(reference.kind.as_str(), KIND_NAME | KIND_IMPORT | KIND_TYPE)
}

#[cfg(test)]
mod tests;
