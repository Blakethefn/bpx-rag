use super::languages::grammar_for_path;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) fn infer_root(target: &Path) -> Result<PathBuf> {
    let parent = target.parent().context("file has no parent directory")?;
    for ancestor in parent.ancestors() {
        if has_project_marker(ancestor) {
            return ancestor.canonicalize().with_context(|| {
                format!("could not resolve inferred root {}", ancestor.display())
            });
        }
    }
    parent
        .canonicalize()
        .with_context(|| format!("could not resolve file directory {}", parent.display()))
}

fn has_project_marker(directory: &Path) -> bool {
    const MARKERS: &[&str] = &[
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "go.mod",
        "go.work",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "pubspec.yaml",
        "Directory.Build.props",
        "Directory.Build.targets",
        "CMakeLists.txt",
        "meson.build",
        "Makefile",
        "Package.swift",
    ];
    if MARKERS.iter().any(|marker| directory.join(marker).exists()) {
        return true;
    }
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "sln" | "slnx" | "csproj" | "fsproj" | "vbproj"
                    )
                })
        })
    })
}

pub(super) fn is_supported(path: &Path) -> bool {
    grammar_for_path(path).is_some()
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
