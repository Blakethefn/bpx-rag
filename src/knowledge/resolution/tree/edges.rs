use std::collections::{HashMap, HashSet};

use super::super::paths::{join_rel, normalize_rel, parent_dir};
use super::layout::module_base;
use super::ModRef;

mod unresolved;

pub(super) use unresolved::unresolved_mod_candidates;

type ModuleTree = (HashMap<i64, HashMap<String, i64>>, HashMap<i64, Vec<i64>>);

pub(super) fn module_tree(
    modules: &[ModRef],
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
) -> ModuleTree {
    let mut grouped: HashMap<(i64, &str), Vec<&ModRef>> = HashMap::new();
    for module in modules {
        if !rust_files.contains(&module.file_id) {
            continue;
        }
        grouped
            .entry((module.file_id, module.name.as_str()))
            .or_default()
            .push(module);
    }

    let mut children: HashMap<i64, HashMap<String, i64>> = HashMap::new();
    let mut parents: HashMap<i64, Vec<i64>> = HashMap::new();
    for ((file_id, name), refs) in grouped {
        let mut targets = HashSet::new();
        let mut blocked = false;
        for module in refs {
            match module_targets(module, files_by_path, packages, rust_files) {
                Targets::None => {}
                Targets::One(target) => {
                    targets.insert(target);
                }
                Targets::Blocked => blocked = true,
            }
        }
        if blocked || targets.len() != 1 {
            continue;
        }
        let Some(target) = targets.into_iter().next() else {
            continue;
        };
        children
            .entry(file_id)
            .or_default()
            .insert(name.to_string(), target);
        let listed = parents.entry(target).or_default();
        if !listed.contains(&file_id) {
            listed.push(file_id);
        }
    }
    (children, parents)
}

pub(super) enum Targets {
    None,
    One(i64),
    Blocked,
}

pub(super) fn module_targets(
    module: &ModRef,
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
) -> Targets {
    let package = packages.get(&module.file_id);
    let accept = |rel: String| -> Option<i64> {
        let id = *files_by_path.get(rel.as_str())?;
        if id == module.file_id || !rust_files.contains(&id) {
            return None;
        }
        (packages.get(&id) == package).then_some(id)
    };
    if let Some(path) = module.qualifier.as_deref().filter(|path| !path.is_empty()) {
        let rel = normalize_rel(&join_rel(&parent_dir(&module.from_rel), path));
        return match accept(rel) {
            Some(id) => Targets::One(id),
            None => Targets::None,
        };
    }
    let base = module_base(&module.from_rel);
    let mut hits = Vec::new();
    for suffix in [
        format!("{}.rs", module.name),
        format!("{}/mod.rs", module.name),
    ] {
        let rel = normalize_rel(&join_rel(&base, &suffix));
        if let Some(id) = accept(rel) {
            hits.push(id);
        }
    }
    hits.sort_unstable();
    hits.dedup();
    match hits.as_slice() {
        [] => Targets::None,
        [id] => Targets::One(*id),
        _ => Targets::Blocked,
    }
}
