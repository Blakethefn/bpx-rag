use std::collections::{HashMap, HashSet};

use super::super::paths::parent_dir;
use super::edges::{module_tree, unresolved_mod_candidates};
use super::{is_rust_path, ModRef, Symbol};

mod roots;

impl super::Tree {
    pub(super) fn build(
        files: Vec<(i64, String)>,
        symbols: Vec<Symbol>,
        modules: Vec<ModRef>,
        entries: Vec<(Option<i64>, String, String)>,
    ) -> Self {
        let files_by_id: HashMap<i64, String> = files.into_iter().collect();
        let files_by_path: HashMap<&str, i64> = files_by_id
            .iter()
            .map(|(id, path)| (path.as_str(), *id))
            .collect();
        let cargo_dirs = cargo_directories(&files_by_id);
        let packages: HashMap<i64, Option<String>> = files_by_id
            .iter()
            .map(|(id, path)| (*id, package_of(path, &cargo_dirs)))
            .collect();
        let rust_files: HashSet<i64> = files_by_id
            .iter()
            .filter(|(_, path)| is_rust_path(path))
            .map(|(id, _)| *id)
            .collect();

        let mut symbols_by_id = HashMap::new();
        let mut symbols_by_file: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbols_by_id.insert(symbol.id, index);
            symbols_by_file
                .entry(symbol.file_id)
                .or_default()
                .push(index);
        }
        let has_macro = symbols.iter().any(|symbol| symbol.kind == "macro");

        let (children, parents) = module_tree(&modules, &files_by_path, &packages, &rust_files);
        let (library_roots, program_roots, program_entries, test_roots, test_names) =
            roots::roots(&entries, &rust_files);
        let root_ids: HashSet<i64> = library_roots
            .iter()
            .copied()
            .chain(program_roots.iter().copied())
            .collect();
        let mut membership = root_ids.clone();
        membership.extend(test_roots);
        let library_files = roots::reachable(&library_roots, &children);
        let program_files = roots::reachable(&program_roots, &children);
        let crate_of = roots::assign_crates(&root_ids, &children);
        let (inline_names, has_inline_module) = roots::inline_modules(&symbols, &modules, &rust_files);
        let unresolved_candidates =
            unresolved_mod_candidates(&modules, &files_by_path, &packages, &rust_files, &children);

        Self {
            symbols,
            symbols_by_id,
            symbols_by_file,
            rust_files,
            children,
            parents,
            crate_of,
            roots: root_ids,
            library_files,
            program_files,
            program_entries,
            membership,
            unresolved_candidates,
            test_names,
            inline_names,
            has_macro,
            has_inline_module,
            paths: files_by_id,
            packages,
        }
    }
}

const ENTRY_PROGRAM: &str = "program";
const ENTRY_TEST: &str = "test";
const ENTRY_LIBRARY: &str = "library_root";

pub(crate) const ENTRY_KINDS: &[&str] = &[ENTRY_PROGRAM, ENTRY_TEST, ENTRY_LIBRARY];


fn cargo_directories(files: &HashMap<i64, String>) -> Vec<String> {
    let mut dirs: Vec<String> = files
        .values()
        .filter(|path| path.rsplit('/').next().unwrap_or(path) == "Cargo.toml")
        .map(|path| parent_dir(path))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn package_of(rel: &str, cargo_dirs: &[String]) -> Option<String> {
    let dir = parent_dir(rel);
    let mut best: Option<&str> = None;
    for package in cargo_dirs {
        let inside =
            package.is_empty() || dir == *package || dir.starts_with(&format!("{package}/"));
        if inside
            && best
                .map(|current| package.len() > current.len())
                .unwrap_or(true)
        {
            best = Some(package);
        }
    }
    best.map(str::to_string)
}
