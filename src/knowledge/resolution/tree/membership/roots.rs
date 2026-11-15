use std::collections::{HashMap, HashSet, VecDeque};

use super::super::{ident, ModRef, Symbol};
use super::{ENTRY_LIBRARY, ENTRY_PROGRAM, ENTRY_TEST};

type Roots = (
    Vec<i64>,
    Vec<i64>,
    HashSet<i64>,
    Vec<i64>,
    HashMap<i64, HashSet<String>>,
);

pub(super) fn roots(entries: &[(Option<i64>, String, String)], rust_files: &HashSet<i64>) -> Roots {
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

pub(super) fn reachable(roots: &[i64], children: &HashMap<i64, HashMap<String, i64>>) -> HashSet<i64> {
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

pub(super) fn assign_crates(
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

pub(super) fn inline_modules(
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
