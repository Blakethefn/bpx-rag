use std::collections::{HashMap, HashSet, VecDeque};

use super::edges::{module_tree, unresolved_mod_candidates};
use super::layout::parent_dir;
use super::{ident, is_rust_path, ModRef, Symbol};

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
            roots(&entries, &rust_files);
        let root_ids: HashSet<i64> = library_roots
            .iter()
            .copied()
            .chain(program_roots.iter().copied())
            .collect();
        let mut membership = root_ids.clone();
        membership.extend(test_roots);
        let library_files = reachable(&library_roots, &children);
        let program_files = reachable(&program_roots, &children);
        let crate_of = assign_crates(&root_ids, &children);
        let (inline_names, has_inline_module) = inline_modules(&symbols, &modules, &rust_files);
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

type Roots = (
    Vec<i64>,
    Vec<i64>,
    HashSet<i64>,
    Vec<i64>,
    HashMap<i64, HashSet<String>>,
);

fn roots(entries: &[(Option<i64>, String, String)], rust_files: &HashSet<i64>) -> Roots {
    let mut library = Vec::new();
    let mut program = Vec::new();
    let mut program_entries = HashSet::new();
    let mut test_roots = Vec::new();
    let mut test_names: HashMap<i64, HashSet<String>> = HashMap::new();
    for (target, kind, reason) in entries {
        let Some(target) = target.filter(|id| rust_files.contains(id)) else {
            continue;
        };
        match kind.as_str() {
            ENTRY_LIBRARY => library.push(target),
            ENTRY_PROGRAM => {
                program.push(target);
                program_entries.insert(target);
            }
            ENTRY_TEST => {
                test_roots.push(target);
                if let Some(name) = reason
                    .strip_prefix("#[test] ")
                    .filter(|name| !name.is_empty())
                {
                    test_names
                        .entry(target)
                        .or_default()
                        .insert(ident(name).to_string());
                }
            }
            _ => {}
        }
    }
    library.sort_unstable();
    library.dedup();
    program.sort_unstable();
    program.dedup();
    test_roots.sort_unstable();
    test_roots.dedup();
    (library, program, program_entries, test_roots, test_names)
}

fn reachable(roots: &[i64], children: &HashMap<i64, HashMap<String, i64>>) -> HashSet<i64> {
    let mut seen = HashSet::new();
    let mut queue: VecDeque<i64> = roots.iter().copied().collect();
    while let Some(file) = queue.pop_front() {
        if !seen.insert(file) {
            continue;
        }
        if let Some(kids) = children.get(&file) {
            queue.extend(kids.values().copied());
        }
    }
    seen
}

/// A file that is a root uses itself. Another root's walk stops at that file, so `crate::`
/// does not jump from a binary root into a library root.
fn assign_crates(
    roots: &HashSet<i64>,
    children: &HashMap<i64, HashMap<String, i64>>,
) -> HashMap<i64, i64> {
    let mut reachers: HashMap<i64, Vec<i64>> = HashMap::new();
    for &root in roots {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        while let Some(file) = queue.pop_front() {
            if !seen.insert(file) {
                continue;
            }
            reachers.entry(file).or_default().push(root);
            if file != root && roots.contains(&file) {
                continue;
            }
            if let Some(kids) = children.get(&file) {
                queue.extend(kids.values().copied());
            }
        }
    }
    let mut crate_of: HashMap<i64, i64> = roots.iter().map(|root| (*root, *root)).collect();
    for (file, reached_by) in reachers {
        if roots.contains(&file) {
            continue;
        }
        if reached_by.len() == 1 {
            crate_of.insert(file, reached_by[0]);
        }
    }
    crate_of
}

fn inline_modules(
    symbols: &[Symbol],
    modules: &[ModRef],
    rust_files: &HashSet<i64>,
) -> (HashMap<i64, HashSet<String>>, bool) {
    let mut declared: HashMap<i64, HashSet<String>> = HashMap::new();
    for module in modules {
        if rust_files.contains(&module.file_id) {
            declared
                .entry(module.file_id)
                .or_default()
                .insert(module.name.clone());
        }
    }
    let mut inline_names: HashMap<i64, HashSet<String>> = HashMap::new();
    for symbol in symbols {
        if symbol.kind != "mod" || !rust_files.contains(&symbol.file_id) {
            continue;
        }
        let name = ident(&symbol.name);
        let declared_here = declared
            .get(&symbol.file_id)
            .is_some_and(|names| names.contains(name));
        if !declared_here {
            inline_names
                .entry(symbol.file_id)
                .or_default()
                .insert(name.to_string());
        }
    }
    let has_inline = inline_names.values().any(|names| !names.is_empty());
    (inline_names, has_inline)
}

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

/// `None` is the single group of files that sit under no `Cargo.toml`.
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
