use std::collections::HashMap;

use super::super::paths::{path_keyword, PathKeyword};
use super::super::tree::{ident, Tree};
use super::{Outcome, Space};

type Bindings = HashMap<i64, HashMap<String, Vec<Vec<i64>>>>;

mod path;

pub(super) use path::find_path;

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

pub(super) fn single_segment(tree: &Tree, from: i64, name: &str, space: Option<Space>) -> Vec<i64> {
    match path_keyword(name) {
        Some(PathKeyword::Crate) => {
            let Some(root) = tree.crate_root(from) else {
                return Vec::new();
            };
            module_symbols(tree, root, from, space)
        }
        Some(PathKeyword::SelfModule) => module_symbols(tree, from, from, space),
        Some(PathKeyword::Super) => {
            let Some(parent) = tree.super_module(from) else {
                return Vec::new();
            };
            module_symbols(tree, parent, from, space)
        }
        None => {
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
