use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

use super::files::is_binary;
use super::policy::{is_sensitive_path, is_sensitive_source_root};
use super::SkippedFile;
use crate::knowledge::scope;

/// A file that could not be read, with the operating system's reason.
fn unreadable_file(rel_path: &str, error: &dyn std::fmt::Display) -> SkippedFile {
    SkippedFile {
        rel_path: rel_path.to_string(),
        reason: format!("unreadable: {error}"),
    }
}

/// Walk a knowledge source directory, returning files keyed by relative path.
/// Respects .gitignore, skips binary/empty/sensitive files, and applies the glob filter.
/// A traversal error aborts the sync so reconciliation cannot mistake an incomplete scan
/// for deleted files. A file that cannot be opened or inspected is left out of `files`, and
/// `walk_source_dir_with_report` names it with the reason.
pub(crate) fn walk_source_dir(
    base_path: &Path,
    patterns: &[String],
) -> Result<BTreeMap<String, PathBuf>> {
    Ok(walk_source_dir_with_report(base_path, patterns)?.files)
}

/// Files keyed by relative path. A `BTreeMap` keeps sync order (and so chunk
/// and file rowids, which tie-break search results) the same across runs.
pub(crate) struct SourceWalkReport {
    pub files: BTreeMap<String, PathBuf>,
    pub sensitive_files_skipped: usize,
    /// Files the walk could not open or inspect, keyed like `files`, with the reason.
    pub unreadable: Vec<SkippedFile>,
}

pub(crate) fn walk_source_dir_with_report(
    base_path: &Path,
    patterns: &[String],
) -> Result<SourceWalkReport> {
    if !base_path.is_dir() {
        return Err(anyhow::anyhow!(
            "knowledge source path is not a directory: {}",
            base_path.display()
        ));
    }
    let canonical_root = base_path
        .canonicalize()
        .with_context(|| format!("could not resolve knowledge source {}", base_path.display()))?;
    if is_sensitive_source_root(&canonical_root) {
        anyhow::bail!("refusing to index a local credential-store directory");
    }

    let mut builder = WalkBuilder::new(base_path);
    builder
        .hidden(false) // Don't skip hidden files (dotfiles may be relevant)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    if let Some(overrides) = scope::build_overrides(base_path, patterns)? {
        builder.overrides(overrides);
    }

    let mut files = BTreeMap::new();
    let mut sensitive_files_skipped = 0;
    let mut unreadable = Vec::new();

    for entry in builder.build() {
        let entry = entry.context("failed while traversing knowledge source")?;

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let abs_path = entry.path().to_path_buf();
        if is_sensitive_path(base_path, &abs_path) {
            sensitive_files_skipped += 1;
            continue;
        }

        let rel_path = abs_path
            .strip_prefix(base_path)
            .unwrap_or(&abs_path)
            .to_string_lossy()
            .to_string();

        // A file that cannot be opened or inspected is skipped with a reason. Only a failure
        // of the traversal itself (above) is fatal, because it hides what a directory holds.
        // Skip binary files (null byte in first 1KB)
        match is_binary(&abs_path) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                unreadable.push(unreadable_file(&rel_path, &error));
                continue;
            }
        }

        // Skip empty files
        match fs::metadata(&abs_path) {
            Ok(metadata) if metadata.len() == 0 => continue,
            Ok(_) => {}
            Err(error) => {
                unreadable.push(unreadable_file(&rel_path, &error));
                continue;
            }
        }

        files.insert(rel_path, abs_path);
    }

    Ok(SourceWalkReport {
        files,
        sensitive_files_skipped,
        unreadable,
    })
}
