use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

use super::files::is_binary;
use super::policy::{is_sensitive_path, is_sensitive_source_root};
use super::SkippedFile;
use crate::knowledge::scope;

fn unreadable_file(rel_path: &str, error: &dyn std::fmt::Display) -> SkippedFile {
    SkippedFile {
        rel_path: rel_path.to_string(),
        reason: format!("unreadable: {error}"),
    }
}

pub(crate) fn walk_source_dir(
    base_path: &Path,
    patterns: &[String],
) -> Result<BTreeMap<String, PathBuf>> {
    Ok(walk_source_dir_with_report(base_path, patterns)?.files)
}

pub(crate) struct SourceWalkReport {
    pub files: BTreeMap<String, PathBuf>,
    pub sensitive_files_skipped: usize,
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
        .hidden(false)
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

        match is_binary(&abs_path) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                unreadable.push(unreadable_file(&rel_path, &error));
                continue;
            }
        }

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
