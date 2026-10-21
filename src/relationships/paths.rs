use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) const SOURCE_EXTENSIONS: &[&str] = &["md", "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs"];

pub(super) fn infer_root(target: &Path) -> Result<PathBuf> {
    let parent = target.parent().context("file has no parent directory")?;
    for ancestor in parent.ancestors() {
        if [".git", "Cargo.toml", "package.json", "pyproject.toml"]
            .iter()
            .any(|marker| ancestor.join(marker).exists())
        {
            return ancestor.canonicalize().with_context(|| {
                format!("could not resolve inferred root {}", ancestor.display())
            });
        }
    }
    parent
        .canonicalize()
        .with_context(|| format!("could not resolve file directory {}", parent.display()))
}

pub(super) fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| SOURCE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub(super) fn is_excluded_directory(path: &Path) -> bool {
    const EXCLUDED: &[&str] = &[".git", "node_modules", "target", "dist", "build", ".next"];
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| EXCLUDED.contains(&name))
        .unwrap_or(false)
}

pub(super) fn display_relative(path: &Path, root: &Path) -> Result<String> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .with_context(|| {
            format!(
                "{} is outside project root {}",
                path.display(),
                root.display()
            )
        })
}

pub(super) fn canonical_file_under(path: &Path, root: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    if canonical.starts_with(root) && canonical.is_file() {
        Some(canonical)
    } else {
        None
    }
}
