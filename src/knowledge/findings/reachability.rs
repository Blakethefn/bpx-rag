//! Unreachable Rust files in a package that has a membership root.
//!
//! A membership root is a program, library, or test entry, including an implicit Cargo target.
//! The walk follows in-memory module children, so a `#[path]` edge counts. A file reached from
//! a test, example, or bench root is not an unused file. A package with an unsealed Rust file
//! publishes no file findings. An unresolved `mod` candidate is withheld here and named
//! `{path}:unsealed` by analysis; it stays out of the symbol seal.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::knowledge::analysis::{FileRow, ANALYSIS_VERSION};
use crate::knowledge::resolution::{is_rust_path, Tree};

const DETACHED: &str = "detached module";
const UNDECLARED: &str = "not declared by a module";

pub(crate) fn write(
    conn: &Connection,
    source_id: i64,
    generation: i64,
    fingerprint: &str,
    files: &[FileRow],
    tree: &Tree,
    unsealed: &HashSet<i64>,
) -> Result<()> {
    let reached = reached_from(tree);
    let rooted_packages: HashSet<&str> = tree
        .membership_roots()
        .iter()
        .filter_map(|id| tree.package_dir(*id))
        .collect();
    let component_of = components(tree);
    let candidates = tree.unresolved_mod_candidates();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_findings
            (source_id, kind, file_id, symbol_id, reason, analysis_version, evidence_generation, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut file_ids: Vec<&FileRow> = files.iter().collect();
    file_ids.sort_by(|left, right| {
        left.rel_path
            .cmp(&right.rel_path)
            .then(left.id.cmp(&right.id))
    });
    for file in file_ids {
        if !is_rust_path(&file.rel_path) {
            continue;
        }
        let Some(package) = tree.package_dir(file.id) else {
            continue;
        };
        if !rooted_packages.contains(package)
            || !tree.package_is_sealed(file.id, unsealed)
            || candidates.contains(&file.id)
            || reached.contains(&file.id)
        {
            continue;
        }
        let reason = if !tree.has_module_edge(file.id) {
            UNDECLARED
        } else if component_has_root(&component_of, tree, file.id) {
            continue;
        } else {
            DETACHED
        };
        insert.execute(params![
            source_id,
            "unreachable_file",
            file.id,
            None::<i64>,
            reason,
            ANALYSIS_VERSION,
            generation,
            fingerprint
        ])?;
    }
    Ok(())
}

fn reached_from(tree: &Tree) -> HashSet<i64> {
    let mut seen = HashSet::new();
    let mut queue: VecDeque<i64> = tree.membership_roots().iter().copied().collect();
    while let Some(file) = queue.pop_front() {
        if !seen.insert(file) {
            continue;
        }
        queue.extend(tree.module_child_ids(file));
    }
    seen
}

fn components(tree: &Tree) -> HashMap<i64, usize> {
    let mut component_of = HashMap::new();
    let mut next = 0usize;
    for file in tree.rust_file_ids() {
        if component_of.contains_key(&file) {
            continue;
        }
        let id = next;
        next += 1;
        let mut queue = VecDeque::from([file]);
        while let Some(current) = queue.pop_front() {
            if component_of.insert(current, id).is_some() {
                continue;
            }
            for neighbor in module_neighbors(tree, current) {
                if !component_of.contains_key(&neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
    }
    component_of
}

fn module_neighbors(tree: &Tree, file: i64) -> Vec<i64> {
    let mut neighbors = Vec::new();
    neighbors.extend(tree.module_child_ids(file));
    neighbors.extend(tree.parents_of(file).iter().copied());
    neighbors
}

fn component_has_root(component_of: &HashMap<i64, usize>, tree: &Tree, file: i64) -> bool {
    let Some(id) = component_of.get(&file) else {
        return false;
    };
    tree.membership_roots()
        .iter()
        .any(|root| component_of.get(root) == Some(id))
}
