use super::super::super::paths::is_path_keyword;
use super::super::super::tree::{path_segments, walk_modules, Tree, Walk};
use super::super::Space;
use super::{dedup, in_space, items_named, single_segment, Bindings, Found};

pub(in super::super) fn find_path(
    tree: &Tree,
    bindings: Option<&Bindings>,
    from: i64,
    path: &str,
    space: Option<Space>,
) -> Found {
    let Some(segments) = path_segments(path) else {
        return if path.trim().starts_with("::") {
            Found::External
        } else {
            Found::Failed
        };
    };
    if segments.len() == 1 {
        let name = segments[0];
        let ids = single_segment(tree, from, name, space);
        if !ids.is_empty() {
            return Found::Symbols {
                ids,
                force_ambiguous: false,
            };
        }
        return if is_path_keyword(name) || tree.child(from, name).is_some() {
            Found::Failed
        } else {
            Found::External
        };
    }
    let name = segments[segments.len() - 1];
    let module = match walk_modules(tree, from, &segments[..segments.len() - 1]) {
        Walk::File(file) => file,
        Walk::External => return Found::External,
        Walk::Failed => return Found::Failed,
    };
    let mut ids = items_named(tree, module, name, from, space);
    let mut force_ambiguous = false;
    if let Some(bindings) = bindings {
        if let Some(sets) = bindings.get(&module).and_then(|names| names.get(name)) {
            force_ambiguous = sets.len() >= 2;
            for set in sets {
                ids.extend(set.iter().copied().filter(|id| {
                    space.is_none_or(|space| in_space(&tree.symbol(*id).kind, space))
                }));
            }
        }
    }
    let ids = dedup(ids);
    if ids.is_empty() {
        Found::Failed
    } else {
        Found::Symbols {
            ids,
            force_ambiguous,
        }
    }
}
