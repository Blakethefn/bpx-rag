use std::collections::HashMap;

use super::super::tree::{ident, Symbol, Tree};
use super::lookup::{find_path, is_data_type, symbols_of};
use super::{resolve_bare, Outcome, Space};

pub(super) fn unique_type(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    from: i64,
    path: &str,
) -> Option<i64> {
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

pub(super) fn type_in_crate(tree: &Tree, type_file: i64, from: i64) -> bool {
    type_file == from || tree.same_crate(type_file, from)
}

pub(super) fn methods_of(
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

pub(super) fn container_names_type(
    tree: &Tree,
    bindings: &HashMap<i64, HashMap<String, Vec<Vec<i64>>>>,
    symbol: &Symbol,
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

pub(super) fn methods_in_file(tree: &Tree, file_id: i64, name: &str) -> Vec<i64> {
    tree.symbols_in(file_id)
        .filter(|symbol| tree.is_method(symbol) && ident(&symbol.name) == name)
        .map(|symbol| symbol.id)
        .collect()
}
