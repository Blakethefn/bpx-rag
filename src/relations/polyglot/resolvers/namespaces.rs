use std::path::{Path, PathBuf};

use super::super::super::super::types::SourceFile;

pub(in crate::relations::polyglot::resolve) fn jvm_targets(
    path: &Path,
    wildcard: bool,
    files: &[SourceFile],
    extensions: &[&str],
) -> Vec<PathBuf> {
    let suffix_paths: Vec<PathBuf> = extensions
        .iter()
        .map(|extension| path.with_extension(*extension))
        .collect();
    files
        .iter()
        .filter(|file| {
            wildcard
                && file
                    .path
                    .parent()
                    .is_some_and(|parent| parent.ends_with(path))
        })
        .chain(files.iter().filter(|file| {
            !wildcard
                && suffix_paths
                    .iter()
                    .any(|suffix| file.path.ends_with(suffix))
        }))
        .filter(|file| {
            file.path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        })
        .map(|file| file.path.clone())
        .collect()
}

pub(in crate::relations::polyglot::resolve) fn namespace_targets(
    path: &Path,
    files: &[SourceFile],
    extensions: &[&str],
) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|file| {
            file.path
                .parent()
                .is_some_and(|parent| parent.ends_with(path))
        })
        .filter(|file| {
            file.path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        })
        .map(|file| file.path.clone())
        .collect()
}

#[cfg(test)]
mod tests;
