use std::collections::HashMap;

mod candidates;
mod rel;

pub(super) use rel::{extension, file_stem, join_rel, normalize_rel, parent_dir};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathKeyword {
    Crate,
    SelfModule,
    Super,
}

pub(super) const PATH_KEYWORDS: &[(&str, PathKeyword)] = &[
    ("crate", PathKeyword::Crate),
    ("self", PathKeyword::SelfModule),
    ("super", PathKeyword::Super),
];

pub(super) fn path_keyword(name: &str) -> Option<PathKeyword> {
    PATH_KEYWORDS
        .iter()
        .find_map(|&(keyword, path_keyword)| (keyword == name).then_some(path_keyword))
}

pub(super) fn is_path_keyword(name: &str) -> bool {
    path_keyword(name).is_some()
}

pub(super) fn is_directory_module_stem(stem: &str) -> bool {
    matches!(stem, "lib" | "main" | "mod")
}

pub(super) fn module_file(
    from_rel: &str,
    specifier: &str,
    files: &HashMap<&str, i64>,
    from_file_id: i64,
) -> Option<i64> {
    for candidate in candidates::module_candidates(from_rel, specifier) {
        if let Some(&file_id) = files.get(candidate.as_str()) {
            if file_id != from_file_id {
                return Some(file_id);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
