use std::collections::HashMap;

use super::super::paths::is_path_keyword;
use super::super::tree::{path_segments, Tree};
use super::super::StoredReference;
use super::lookup::{find_path, in_space, items_named, outcome, symbol_outcome};
use super::methods::{methods_in_file, methods_of, unique_type};
use super::{Outcome, Space};

pub(super) fn resolve_bare(
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

pub(super) fn resolve_qualified(
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

pub(super) fn entered_module(tree: &Tree, from: i64, path: &str) -> bool {
    let Some(segments) = path_segments(path) else {
        return false;
    };
    is_path_keyword(segments[0]) || tree.child(from, segments[0]).is_some()
}
