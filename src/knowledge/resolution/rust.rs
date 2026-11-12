use std::collections::HashMap;

use super::paths::is_path_keyword;
use super::tree::{ident, path_segments, Tree};
use super::StoredReference;
use crate::chunk::evidence::{KIND_IMPORT, KIND_NAME, KIND_TYPE};

mod lookup;
mod methods;

use lookup::{
    find_path, in_space, items_named, outcome, single_segment, symbol_outcome, symbols_of,
};
use methods::{methods_in_file, methods_of, unique_type};

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
            resolve_qualified(tree, &bindings, reference, space)
        } else {
            resolve_bare(
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

fn resolve_bare(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    file_id: i64,
    name: &str,
    space: Space,
) -> Outcome {
    let sets = bindings.get(&file_id).and_then(|names| names.get(name));
    let mut ids = items_named(tree, file_id, name, file_id, Some(space));
    if let Some(sets) = sets {
        for set in sets {
            ids.extend(
                set.iter()
                    .copied()
                    .filter(|id| in_space(&tree.symbol(*id).kind, space)),
            );
        }
    }
    outcome(ids, sets.is_some_and(|sets| sets.len() >= 2))
}

fn resolve_qualified(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    reference: &StoredReference,
    space: Space,
) -> Outcome {
    let qualifier = reference.qualifier.as_deref().unwrap_or("");
    let name = reference.specifier.as_str();
    if qualifier.starts_with("::") {
        return Outcome::Unresolved;
    }
    if qualifier == "Self" {
        return outcome(methods_in_file(tree, reference.file_id, name), true);
    }
    let full = format!("{qualifier}::{name}");
    if let Some(resolved) = symbol_outcome(find_path(
        tree,
        Some(bindings),
        reference.file_id,
        &full,
        Some(space),
    )) {
        return resolved;
    }
    if let Some(type_id) = unique_type(tree, bindings, reference.file_id, qualifier) {
        return outcome(
            methods_of(tree, bindings, type_id, name, reference.file_id),
            false,
        );
    }
    if entered_module(tree, reference.file_id, &full) {
        return Outcome::Unresolved;
    }
    let first = qualifier.split("::").find(|segment| !segment.is_empty());
    if first.is_some_and(|segment| name_in_scope(tree, bindings, reference.file_id, segment)) {
        return outcome(methods_in_file(tree, reference.file_id, name), true);
    }
    Outcome::Unresolved
}

fn name_in_scope(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    file_id: i64,
    name: &str,
) -> bool {
    if !items_named(tree, file_id, name, file_id, None).is_empty() {
        return true;
    }
    bindings
        .get(&file_id)
        .and_then(|names| names.get(name))
        .is_some_and(|sets| sets.iter().any(|set| !set.is_empty()))
}

fn entered_module(tree: &Tree, from: i64, path: &str) -> bool {
    let Some(segments) = path_segments(path) else {
        return false;
    };
    is_path_keyword(segments[0]) || tree.child(from, segments[0]).is_some()
}

#[cfg(test)]
mod tests;
