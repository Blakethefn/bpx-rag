use std::path::{Path, PathBuf};

use super::super::super::super::paths::canonical_file_under;

pub(in crate::relationships::polyglot::resolve) fn add_file_candidates(
    base: &Path,
    root: &Path,
    extensions: &[&str],
    output: &mut Vec<PathBuf>,
) {
    if let Some(path) = canonical_file_under(base, root) {
        output.push(path);
    }
    if base.extension().is_none() {
        for extension in extensions {
            if let Some(path) = canonical_file_under(&base.with_extension(extension), root) {
                output.push(path);
            }
        }
    }
}
