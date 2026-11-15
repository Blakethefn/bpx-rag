use std::collections::HashMap;

use super::tree::{ident, Tree};
use super::StoredReference;
use crate::chunk::evidence::{KIND_IMPORT, KIND_NAME, KIND_TYPE};

mod lookup;
mod methods;
mod qualify;

use lookup::{find_path, outcome, single_segment, symbols_of};

#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Unresolved,
    Resolved(i64),
    Ambiguous(Vec<i64>),
}

#[derive(Clone, Copy)]
enum Space {
    Value,
    Type,
}

pub(super) fn resolve(tree: &Tree, references: &[StoredReference]) -> HashMap<i64, Outcome> {
    let mut bindings: HashMap<i64, HashMap<String, Vec<Vec<i64>>>> = HashMap::new();
    let mut outcomes = HashMap::new();
    for reference in references {
        if !tree.is_rust_file(reference.file_id) || reference.kind != KIND_IMPORT {
            continue;
        }
        if reference.specifier == "*" {
            let ids = glob_targets(tree, reference);
            bind_names(tree, &mut bindings, reference.file_id, &ids);
            outcomes.insert(reference.id, outcome(ids, false));
            continue;
        }
        let ids = import_targets(tree, reference);
        bindings
            .entry(reference.file_id)
            .or_default()
            .entry(reference.specifier.clone())
            .or_default()
            .push(ids.clone());
        outcomes.insert(reference.id, outcome(ids, false));
    }
    for reference in references {
        if !tree.is_rust_file(reference.file_id) {
            continue;
        }
        let space = match reference.kind.as_str() {
            KIND_NAME => Space::Value,
            KIND_TYPE => Space::Type,
            _ => continue,
        };
        let resolved = if reference.qualifier.is_some() {
            qualify::resolve_qualified(tree, &bindings, reference, space)
        } else {
            qualify::resolve_bare(
                tree,
                &bindings,
                reference.file_id,
                &reference.specifier,
                space,
            )
        };
        outcomes.insert(reference.id, resolved);
    }
    outcomes
}

fn import_targets(tree: &Tree, reference: &StoredReference) -> Vec<i64> {
    let Some(qualifier) = reference.qualifier.as_deref() else {
        return single_segment(tree, reference.file_id, &reference.specifier, None);
    };
    let appended = format!("{qualifier}::{}", reference.specifier);
    if let Some(ids) = symbols_of(find_path(tree, None, reference.file_id, &appended, None)) {
        return ids;
    }
    symbols_of(find_path(tree, None, reference.file_id, qualifier, None))
        .unwrap_or_default()
        .into_iter()
        .filter(|id| tree.symbol(*id).kind != "mod")
        .collect()
}

fn glob_targets(tree: &Tree, reference: &StoredReference) -> Vec<i64> {
    let Some(path) = reference.qualifier.as_deref() else {
        return Vec::new();
    };
    let Some(module) = tree.module_file(reference.file_id, path) else {
        return Vec::new();
    };
    tree.symbols_in(module)
        .filter(|symbol| tree.glob_visible(symbol, reference.file_id, module))
        .map(|symbol| symbol.id)
        .collect()
}

fn bind_names(
    tree: &Tree,
    bindings: &mut HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    file_id: i64,
    ids: &[i64],
) {
    let mut by_name: HashMap<String, Vec<i64>> = HashMap::new();
    for id in ids {
        let name = ident(&tree.symbol(*id).name).to_string();
        by_name.entry(name).or_default().push(*id);
    }
    let local = bindings.entry(file_id).or_default();
    for (name, targets) in by_name {
        local.entry(name).or_default().push(targets);
    }
}


#[cfg(test)]
mod tests;
