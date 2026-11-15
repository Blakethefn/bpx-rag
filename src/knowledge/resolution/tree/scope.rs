use std::collections::HashSet;

use super::{ident, Symbol};

impl super::Tree {
    pub(crate) fn membership_roots(&self) -> &HashSet<i64> {
        &self.membership
    }

    pub(crate) fn unresolved_mod_candidates(&self) -> &HashSet<i64> {
        &self.unresolved_candidates
    }

    pub(crate) fn rust_file_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.rust_files.iter().copied()
    }

    pub(crate) fn package_is_sealed(&self, file_id: i64, unsealed: &HashSet<i64>) -> bool {
        let package = self.packages.get(&file_id);
        self.rust_files
            .iter()
            .all(|other| self.packages.get(other) != package || !unsealed.contains(other))
    }

    pub(crate) fn same_crate(&self, left: i64, right: i64) -> bool {
        match (self.crate_of.get(&left), self.crate_of.get(&right)) {
            (Some(left_root), Some(right_root)) => left_root == right_root,
            _ => false,
        }
    }

    pub(crate) fn on_program_only(&self, file_id: i64) -> bool {
        self.program_files.contains(&file_id) && !self.library_files.contains(&file_id)
    }

    pub(crate) fn is_program_main(&self, symbol: &Symbol) -> bool {
        ident(&symbol.name) == "main"
            && symbol.kind == "function"
            && !self.is_method(symbol)
            && self.program_entries.contains(&symbol.file_id)
    }

    pub(crate) fn is_named_test(&self, symbol: &Symbol) -> bool {
        symbol.kind == "function"
            && self
                .test_names
                .get(&symbol.file_id)
                .is_some_and(|names| names.contains(ident(&symbol.name)))
    }
}
