use std::collections::HashMap;

use super::super::tree::{ident, path_segments, walk_modules, Tree, Walk};
use super::{Outcome, Space};

pub(super) enum Found {
    Symbols {
        ids: Vec<i64>,
        force_ambiguous: bool,
    },
    External,
    Failed,
}

pub(super) fn symbols_of(found: Found) -> Option<Vec<i64>> {
    match found {
        Found::Symbols { ids, .. } if !ids.is_empty() => Some(ids),
        _ => None,
    }
}

pub(super) fn symbol_outcome(found: Found) -> Option<Outcome> {
    match found {
        Found::Symbols {
            ids,
            force_ambiguous,
        } if !ids.is_empty() => Some(outcome(ids, force_ambiguous)),
        _ => None,
    }
}

pub(super) fn find_path(
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

pub(super) fn single_segment(tree: &Tree, from: i64, name: &str, space: Option<Space>) -> Vec<i64> {
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

pub(super) fn items_named(
    tree: &Tree,
    module: i64,
    name: &str,
    from: i64,
    space: Option<Space>,
) -> Vec<i64> {
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

pub(super) fn in_space(kind: &str, space: Space) -> bool {
    match space {
        Space::Value => matches!(
            kind,
            "function" | "const" | "static" | "mod" | "struct" | "enum" | "union"
        ),
        Space::Type => matches!(kind, "struct" | "enum" | "union" | "trait" | "type" | "mod"),
    }
}

pub(super) fn is_data_type(kind: &str) -> bool {
    matches!(kind, "struct" | "enum" | "type")
}

pub(super) fn outcome(ids: Vec<i64>, force_ambiguous: bool) -> Outcome {
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
