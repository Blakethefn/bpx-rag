use anyhow::{bail, Context, Result};
use ignore::WalkBuilder;
use std::fs;
use std::path::Path;

use super::paths::{display_relative, infer_root, is_excluded_directory, is_supported};
use super::types::{FileAnalysis, SourceFile};

mod extract;

pub use extract::project_edges;

const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

pub fn analyze_file(path: &Path, root: Option<&Path>) -> Result<FileAnalysis> {
    let unresolved_path = match root {
        Some(root) if !path.is_absolute() => root.join(path),
        _ => path.to_path_buf(),
    };
    let target = unresolved_path
        .canonicalize()
        .with_context(|| format!("could not resolve file {}", path.display()))?;
    if !target.is_file() {
        bail!("{} is not a regular file", target.display());
    }

    let root = match root {
        Some(path) => {
            let unresolved_root = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            unresolved_root
                .canonicalize()
                .with_context(|| format!("could not resolve project root {}", path.display()))?
        }
        None => infer_root(&target)?,
    };
    if !root.is_dir() {
        bail!("project root {} is not a directory", root.display());
    }
    if !target.starts_with(&root) {
        bail!(
            "file {} is outside project root {}",
            target.display(),
            root.display()
        );
    }
    if !is_supported(&target) {
        bail!("unsupported file type: {}", target.display());
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped = 0usize;
    let mut reached_limit = false;
    let target_metadata = fs::metadata(&target)
        .with_context(|| format!("could not read metadata for {}", target.display()))?;
    if target_metadata.len() > MAX_FILE_BYTES {
        bail!("selected file exceeds the {MAX_FILE_BYTES}-byte analysis limit");
    }
    let target_content = fs::read_to_string(&target)
        .with_context(|| format!("selected file is not readable UTF-8: {}", target.display()))?;
    let mut total_bytes = target_metadata.len();
    files.push(SourceFile {
        path: target.clone(),
        content: target_content,
    });
    let mut builder = WalkBuilder::new(&root);
    builder.hidden(true).git_ignore(true).git_global(true);
    builder.filter_entry(|entry| !is_excluded_directory(entry.path()));

    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                skipped += 1;
                if warnings.is_empty() {
                    warnings.push(format!("Some files could not be read: {error}"));
                }
                continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_supported(entry.path()) {
            continue;
        }
        if entry.path() == target {
            continue;
        }

        if files.len() >= MAX_FILES {
            reached_limit = true;
            break;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if metadata.len() > MAX_FILE_BYTES {
            skipped += 1;
            continue;
        }
        if total_bytes.saturating_add(metadata.len()) > MAX_TOTAL_BYTES {
            reached_limit = true;
            break;
        }
        let content = match fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        total_bytes = total_bytes.saturating_add(metadata.len());
        files.push(SourceFile {
            path: entry.path().to_path_buf(),
            content,
        });
    }

    if reached_limit {
        warnings.push(format!(
            "Scan reached its file or byte limit ({MAX_FILES} files, {MAX_TOTAL_BYTES} bytes); results may be incomplete."
        ));
    }
    if skipped > 0 && warnings.is_empty() {
        warnings.push(format!("Skipped {skipped} unreadable or oversized files."));
    }

    let (outgoing, incoming) = extract::relationships(&target, &root, &files)?;
    Ok(FileAnalysis {
        root: root.display().to_string(),
        file: display_relative(&target, &root)?,
        scanned_files: files.len(),
        outgoing,
        incoming,
        warnings,
    })
}
