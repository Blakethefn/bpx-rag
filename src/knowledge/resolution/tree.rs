//! In-memory Rust module tree for name resolution and file membership.
//!
//! Package boundaries, `mod` edges, and crate roots live here. They are not written onto
//! `module` or `path` resolution rows. File findings walk these children, including `#[path]`.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

mod edges;
mod layout;
mod load;
mod membership;
mod visible;
mod walk;

use layout::extension;
use load::{load_entries, load_files, load_modules, load_symbols};
pub(crate) use membership::ENTRY_KINDS;
pub(in crate::knowledge::resolution) use walk::{path_segments, walk_modules, Walk};

pub(crate) struct Symbol {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
    pub visibility: Option<String>,
}

struct ModRef {
    file_id: i64,
    from_rel: String,
    name: String,
    qualifier: Option<String>,
}

pub(crate) struct Tree {
    symbols: Vec<Symbol>,
    symbols_by_id: HashMap<i64, usize>,
    symbols_by_file: HashMap<i64, Vec<usize>>,
    rust_files: HashSet<i64>,
    children: HashMap<i64, HashMap<String, i64>>,
    parents: HashMap<i64, Vec<i64>>,
    /// Crate root for a file that has exactly one. A root maps to itself.
    crate_of: HashMap<i64, i64>,
    roots: HashSet<i64>,
    library_files: HashSet<i64>,
    program_files: HashSet<i64>,
    program_entries: HashSet<i64>,
    /// Program, library, and test entry targets. Test files stay out of `program_files`.
    membership: HashSet<i64>,
    /// Indexed files an unresolved `mod` could have named. Not part of the symbol seal.
    unresolved_candidates: HashSet<i64>,
    test_names: HashMap<i64, HashSet<String>>,
    inline_names: HashMap<i64, HashSet<String>>,
    has_macro: bool,
    has_inline_module: bool,
    paths: HashMap<i64, String>,
    /// `None` is the one group of files that sit under no `Cargo.toml`.
    packages: HashMap<i64, Option<String>>,
}

pub(crate) fn is_rust_path(path: &str) -> bool {
    extension(path).eq_ignore_ascii_case("rs")
}

pub(crate) fn ident(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

impl Tree {
    pub(crate) fn load(conn: &Connection, source_id: i64) -> Result<Self> {
        let files = load_files(conn, source_id)?;
        let symbols = load_symbols(conn, source_id)?;
        let modules = load_modules(conn, source_id)?;
        let entries = load_entries(conn, source_id)?;
        Ok(Self::build(files, symbols, modules, entries))
    }

    pub(crate) fn is_rust_file(&self, file_id: i64) -> bool {
        self.rust_files.contains(&file_id)
    }

    pub(crate) fn symbol(&self, id: i64) -> &Symbol {
        &self.symbols[self.symbols_by_id[&id]]
    }

    pub(crate) fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub(crate) fn symbols_in(&self, file_id: i64) -> impl Iterator<Item = &Symbol> {
        self.symbols_by_file
            .get(&file_id)
            .into_iter()
            .flat_map(|indexes| indexes.iter().map(|&index| &self.symbols[index]))
    }

    pub(crate) fn child(&self, file_id: i64, name: &str) -> Option<i64> {
        self.children.get(&file_id)?.get(name).copied()
    }

    pub(crate) fn super_module(&self, file_id: i64) -> Option<i64> {
        if self.roots.contains(&file_id) {
            return None;
        }
        let parents = self.parents.get(&file_id)?;
        if parents.len() == 1 {
            Some(parents[0])
        } else {
            None
        }
    }

    pub(crate) fn crate_root(&self, file_id: i64) -> Option<i64> {
        self.crate_of.get(&file_id).copied()
    }

    pub(crate) fn parents_of(&self, file_id: i64) -> &[i64] {
        self.parents.get(&file_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(crate) fn module_child_ids(&self, file_id: i64) -> impl Iterator<Item = i64> + '_ {
        self.children
            .get(&file_id)
            .into_iter()
            .flat_map(|children| children.values().copied())
    }

    pub(crate) fn has_module_edge(&self, file_id: i64) -> bool {
        self.children
            .get(&file_id)
            .is_some_and(|children| !children.is_empty())
            || self
                .parents
                .get(&file_id)
                .is_some_and(|parents| !parents.is_empty())
    }

    /// The nearest `Cargo.toml` directory. `None` means the file sits under no manifest.
    pub(crate) fn package_dir(&self, file_id: i64) -> Option<&str> {
        self.packages
            .get(&file_id)
            .and_then(|package| package.as_deref())
    }

    pub(crate) fn membership_roots(&self) -> &HashSet<i64> {
        &self.membership
    }

    pub(crate) fn unresolved_mod_candidates(&self) -> &HashSet<i64> {
        &self.unresolved_candidates
    }

    pub(crate) fn rust_file_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.rust_files.iter().copied()
    }

    /// Every Rust file in this file's package is outside `unsealed`.
    pub(crate) fn package_is_sealed(&self, file_id: i64, unsealed: &HashSet<i64>) -> bool {
        let package = self.packages.get(&file_id);
        self.rust_files
            .iter()
            .all(|other| self.packages.get(other) != package || !unsealed.contains(other))
    }

    pub(super) fn child_name(&self, parent: i64, child: i64) -> Option<&str> {
        self.children
            .get(&parent)?
            .iter()
            .find_map(|(name, id)| (*id == child).then_some(name.as_str()))
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
