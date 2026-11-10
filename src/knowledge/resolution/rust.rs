//! Rust name, import, and type resolution inside one file's module.
//!
//! A bare name sees local items and `use` bindings in that file. A path walks `crate`, `self`,
//! `super`, and child modules. External paths and prelude names that the file does not define
//! or import stay unresolved, with no candidates in other files.
//!
//! Alias rows and plain imports share columns. The appended path is the plain import. When that
//! path names nothing and the qualifier names a non-module item, the row is an alias and the
//! qualifier is the whole referred path.

use std::collections::HashMap;

use super::tree::{ident, path_segments, walk_modules, Tree, Walk};
use super::StoredReference;
use crate::chunk::evidence::{KIND_IMPORT, KIND_NAME, KIND_TYPE};

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

/// Outcomes for Rust `name`, `import`, and `type` references. Other references are absent.
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
    // A plain import whose parent module exists would have resolved the appended path when the
    // item exists. A qualifier that names a value or type is the alias's full path.
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
    // Two `use` bindings of one local name do not pick one symbol, even when both name it.
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

fn unique_type(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    from: i64,
    path: &str,
) -> Option<i64> {
    // A single segment is a name in this module, including a `use` binding. A longer path walks
    // modules. `find_path` treats a lone segment that is not a child module as external.
    let ids = if path.contains("::") || path.starts_with("::") {
        if path.starts_with("::") {
            return None;
        }
        symbols_of(find_path(
            tree,
            Some(bindings),
            from,
            path,
            Some(Space::Type),
        ))?
    } else {
        match resolve_bare(tree, bindings, from, path, Space::Type) {
            Outcome::Unresolved => return None,
            Outcome::Resolved(id) => vec![id],
            Outcome::Ambiguous(ids) => ids,
        }
    };
    let types: Vec<i64> = ids
        .into_iter()
        .filter(|id| {
            let symbol = tree.symbol(*id);
            is_data_type(&symbol.kind) && type_in_crate(tree, symbol.file_id, from)
        })
        .collect();
    match types.as_slice() {
        [id] => Some(*id),
        _ => None,
    }
}

fn type_in_crate(tree: &Tree, type_file: i64, from: i64) -> bool {
    type_file == from || tree.same_crate(type_file, from)
}

fn methods_of(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    type_id: i64,
    name: &str,
    from: i64,
) -> Vec<i64> {
    tree.symbols()
        .iter()
        .filter(|symbol| ident(&symbol.name) == name)
        .filter(|symbol| tree.is_method(symbol))
        .filter(|symbol| container_names_type(tree, bindings, symbol, type_id))
        .filter(|symbol| {
            symbol.file_id == tree.symbol(type_id).file_id
                || tree.same_crate(symbol.file_id, tree.symbol(type_id).file_id)
        })
        .filter(|symbol| tree.visible(symbol, from))
        .map(|symbol| symbol.id)
        .collect()
}

/// The impl container must name this type from the method's own file, not another type with the
/// same identifier.
fn container_names_type(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    symbol: &super::tree::Symbol,
    type_id: i64,
) -> bool {
    let Some(container) = symbol.container.as_deref() else {
        return false;
    };
    let head = container.split('<').next().unwrap_or(container).trim();
    if head.is_empty() || head.starts_with("::") {
        return false;
    }
    unique_type(tree, bindings, symbol.file_id, head) == Some(type_id)
}

fn methods_in_file(tree: &Tree, file_id: i64, name: &str) -> Vec<i64> {
    tree.symbols_in(file_id)
        .filter(|symbol| tree.is_method(symbol) && ident(&symbol.name) == name)
        .map(|symbol| symbol.id)
        .collect()
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
    match segments[0] {
        "crate" | "self" | "super" => true,
        name => tree.child(from, name).is_some(),
    }
}

enum Found {
    Symbols {
        ids: Vec<i64>,
        force_ambiguous: bool,
    },
    External,
    Failed,
}

fn symbols_of(found: Found) -> Option<Vec<i64>> {
    match found {
        Found::Symbols { ids, .. } if !ids.is_empty() => Some(ids),
        _ => None,
    }
}

fn symbol_outcome(found: Found) -> Option<Outcome> {
    match found {
        Found::Symbols {
            ids,
            force_ambiguous,
        } if !ids.is_empty() => Some(outcome(ids, force_ambiguous)),
        _ => None,
    }
}

fn find_path(
    tree: &Tree,
    bindings: Option<&HashMap<i64, HashMap<String, Vec<Vec<i64>>>>>,
    from: i64,
    path: &str,
    space: Option<Space>,
) -> Found {
    let Some(segments) = path_segments(path) else {
        return if path.trim().starts_with("::") {
            Found::External
        } else {
            Found::Failed
        };
    };
    if segments.len() == 1 {
        let name = segments[0];
        let ids = single_segment(tree, from, name, space);
        if !ids.is_empty() {
            return Found::Symbols {
                ids,
                force_ambiguous: false,
            };
        }
        return if matches!(name, "crate" | "self" | "super") || tree.child(from, name).is_some() {
            Found::Failed
        } else {
            Found::External
        };
    }
    let name = segments[segments.len() - 1];
    let module = match walk_modules(tree, from, &segments[..segments.len() - 1]) {
        Walk::File(file) => file,
        Walk::External => return Found::External,
        Walk::Failed => return Found::Failed,
    };
    let mut ids = items_named(tree, module, name, from, space);
    let mut force_ambiguous = false;
    if let Some(bindings) = bindings {
        if let Some(sets) = bindings.get(&module).and_then(|names| names.get(name)) {
            force_ambiguous = sets.len() >= 2;
            for set in sets {
                ids.extend(set.iter().copied().filter(|id| {
                    space.is_none_or(|space| in_space(&tree.symbol(*id).kind, space))
                }));
            }
        }
    }
    let ids = dedup(ids);
    if ids.is_empty() {
        Found::Failed
    } else {
        Found::Symbols {
            ids,
            force_ambiguous,
        }
    }
}

fn single_segment(tree: &Tree, from: i64, name: &str, space: Option<Space>) -> Vec<i64> {
    match name {
        "crate" => {
            let Some(root) = tree.crate_root(from) else {
                return Vec::new();
            };
            module_symbols(tree, root, from, space)
        }
        "self" => module_symbols(tree, from, from, space),
        "super" => {
            let Some(parent) = tree.super_module(from) else {
                return Vec::new();
            };
            module_symbols(tree, parent, from, space)
        }
        name => {
            if tree.child(from, name).is_none() {
                return Vec::new();
            }
            items_named(tree, from, name, from, space)
                .into_iter()
                .filter(|id| tree.symbol(*id).kind == "mod")
                .collect()
        }
    }
}

fn module_symbols(tree: &Tree, module: i64, from: i64, space: Option<Space>) -> Vec<i64> {
    let mut ids = Vec::new();
    for parent in tree.parents_of(module) {
        if let Some(name) = tree.child_name(*parent, module) {
            ids.extend(
                items_named(tree, *parent, name, from, space)
                    .into_iter()
                    .filter(|id| tree.symbol(*id).kind == "mod"),
            );
        }
    }
    dedup(ids)
}

fn items_named(tree: &Tree, module: i64, name: &str, from: i64, space: Option<Space>) -> Vec<i64> {
    dedup(
        tree.symbols_in(module)
            .filter(|symbol| ident(&symbol.name) == name)
            .filter(|symbol| !tree.is_method(symbol))
            .filter(|symbol| space.is_none_or(|space| in_space(&symbol.kind, space)))
            .filter(|symbol| tree.visible(symbol, from))
            .map(|symbol| symbol.id)
            .collect(),
    )
}

fn in_space(kind: &str, space: Space) -> bool {
    match space {
        Space::Value => matches!(
            kind,
            "function" | "const" | "static" | "mod" | "struct" | "enum" | "union"
        ),
        Space::Type => matches!(kind, "struct" | "enum" | "union" | "trait" | "type" | "mod"),
    }
}

fn is_data_type(kind: &str) -> bool {
    matches!(kind, "struct" | "enum" | "type")
}

fn outcome(ids: Vec<i64>, force_ambiguous: bool) -> Outcome {
    let ids = dedup(ids);
    match (force_ambiguous, ids.as_slice()) {
        (_, []) => Outcome::Unresolved,
        (false, [id]) => Outcome::Resolved(*id),
        (_, many) => Outcome::Ambiguous(many.to_vec()),
    }
}

fn dedup(mut ids: Vec<i64>) -> Vec<i64> {
    ids.sort_unstable();
    ids.dedup();
    ids
}
