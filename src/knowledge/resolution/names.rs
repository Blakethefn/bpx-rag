//! Symbol candidates for one stored reference.

use super::paths::{file_stem, parent_dir};
use super::{StoredReference, StoredSymbol};

pub(super) fn symbol_candidates(reference: &StoredReference, symbols: &[StoredSymbol]) -> Vec<i64> {
    let named: Vec<&StoredSymbol> = symbols
        .iter()
        .filter(|symbol| symbol.name == reference.specifier)
        .collect();
    let Some(qualifier) = reference.qualifier.as_deref() else {
        return named.iter().map(|symbol| symbol.id).collect();
    };
    if is_receiver(qualifier) {
        return named.iter().map(|symbol| symbol.id).collect();
    }
    let last = qualifier
        .rsplit([':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(qualifier);
    named
        .into_iter()
        .filter(|symbol| {
            symbol.container.as_deref() == Some(qualifier)
                || symbol.container.as_deref() == Some(last)
                || file_stem(&symbol.rel_path) == last
                || parent_dir(&symbol.rel_path).ends_with(last)
        })
        .map(|symbol| symbol.id)
        .collect()
}

fn is_receiver(qualifier: &str) -> bool {
    matches!(qualifier, "self" | "Self" | "this" | "cls" | "super")
}
