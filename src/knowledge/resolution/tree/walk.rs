use std::collections::HashSet;

use super::Tree;

pub(in crate::knowledge::resolution) enum Walk {
    File(i64),
    External,
    Failed,
}

pub(in crate::knowledge::resolution) fn walk_modules(
    tree: &Tree,
    from: i64,
    segments: &[&str],
) -> Walk {
    if segments.is_empty() {
        return Walk::Failed;
    }
    let mut seen = HashSet::new();
    let mut module = match segments[0] {
        "crate" => match tree.crate_root(from) {
            Some(root) => root,
            None => return Walk::Failed,
        },
        "self" => from,
        "super" => match tree.super_module(from) {
            Some(parent) => parent,
            None => return Walk::Failed,
        },
        name => match tree.child(from, name) {
            Some(child) => child,
            None => return Walk::External,
        },
    };
    seen.insert(module);
    for segment in &segments[1..] {
        let next = match *segment {
            "self" => module,
            "super" => match tree.super_module(module) {
                Some(parent) => parent,
                None => return Walk::Failed,
            },
            "crate" => return Walk::Failed,
            name => match tree.child(module, name) {
                Some(child) => child,
                None => return Walk::Failed,
            },
        };
        if !seen.insert(next) {
            return Walk::Failed;
        }
        module = next;
    }
    Walk::File(module)
}

pub(in crate::knowledge::resolution) fn path_segments(path: &str) -> Option<Vec<&str>> {
    let path = path.trim();
    if path.is_empty() || path.starts_with("::") {
        return None;
    }
    let segments: Vec<&str> = path
        .split("::")
        .filter(|segment| !segment.is_empty())
        .collect();
    (!segments.is_empty()).then_some(segments)
}
