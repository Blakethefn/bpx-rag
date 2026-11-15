use std::collections::{HashMap, HashSet};

use super::super::super::paths::{join_rel, normalize_rel, parent_dir};
use super::super::layout::module_base;
use super::super::ModRef;

pub(in super::super) fn unresolved_mod_candidates(
    modules: &[ModRef],
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
    children: &HashMap<i64, HashMap<String, i64>>,
) -> HashSet<i64> {
    let mut grouped: HashMap<(i64, &str), Vec<&ModRef>> = HashMap::new();
    for module in modules {
        if rust_files.contains(&module.file_id) {
            grouped
                .entry((module.file_id, module.name.as_str()))
                .or_default()
                .push(module);
        }
    }
    let mut candidates = HashSet::new();
    for ((file_id, name), refs) in grouped {
        if children
            .get(&file_id)
            .is_some_and(|kids| kids.contains_key(name))
        {
            continue;
        }
        let package = packages.get(&file_id);
        for module in refs {
            if let Some(path) = module.qualifier.as_deref().filter(|path| !path.is_empty()) {
                let rel = normalize_rel(&join_rel(&parent_dir(&module.from_rel), path));
                consider_candidate(
                    &mut candidates,
                    files_by_path,
                    packages,
                    rust_files,
                    package,
                    file_id,
                    &rel,
                );
            }
            let base = module_base(&module.from_rel);
            for suffix in [format!("{name}.rs"), format!("{name}/mod.rs")] {
                let rel = normalize_rel(&join_rel(&base, &suffix));
                consider_candidate(
                    &mut candidates,
                    files_by_path,
                    packages,
                    rust_files,
                    package,
                    file_id,
                    &rel,
                );
            }
        }
    }
    candidates
}

fn consider_candidate(
    candidates: &mut HashSet<i64>,
    files_by_path: &HashMap<&str, i64>,
    packages: &HashMap<i64, Option<String>>,
    rust_files: &HashSet<i64>,
    package: Option<&Option<String>>,
    from: i64,
    rel: &str,
) {
    let Some(&id) = files_by_path.get(rel) else {
        return;
    };
    if id != from && rust_files.contains(&id) && packages.get(&id) == package {
        candidates.insert(id);
    }
}
