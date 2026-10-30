use std::path::{Path, PathBuf};

use super::paths::canonical_file_under;
use super::types::{RawReference, SourceFile};

pub(super) fn markdown_references(
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
) -> Vec<RawReference> {
    let mut references = Vec::new();
    let mut in_fence = false;

    for (line_index, line) in source.content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        let bytes = line.as_bytes();
        let mut cursor = 0usize;
        let mut in_inline_code = false;
        while cursor < bytes.len() {
            if bytes[cursor] == b'`' {
                in_inline_code = !in_inline_code;
                cursor += 1;
                continue;
            }
            if !in_inline_code
                && cursor + 1 < bytes.len()
                && bytes[cursor] == b'['
                && bytes[cursor + 1] == b'['
            {
                let start = cursor + 2;
                if let Some(end) = line[start..].find("]]") {
                    let specifier = line[start..start + end]
                        .split('|')
                        .next()
                        .unwrap_or_default()
                        .split('#')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if !specifier.is_empty() {
                        let target = resolve_wikilink(&source.path, root, &specifier, files);
                        references.push(RawReference {
                            kind: "wikilink",
                            specifier,
                            line: line_index + 1,
                            target,
                        });
                    }
                    cursor = start + end + 2;
                    continue;
                }
            }
            cursor += 1;
        }
    }
    references
}

fn resolve_wikilink(
    source: &Path,
    root: &Path,
    specifier: &str,
    files: &[SourceFile],
) -> Option<PathBuf> {
    let target = Path::new(specifier);
    if target.is_absolute() {
        return None;
    }
    let mut candidates = Vec::new();
    let source_parent = source.parent()?;
    candidates.push(source_parent.join(target));
    candidates.push(root.join(target));
    if target.extension().is_none() {
        candidates.push(source_parent.join(target).with_extension("md"));
        candidates.push(root.join(target).with_extension("md"));
    }

    for candidate in candidates {
        if let Some(resolved) = canonical_file_under(&candidate, root) {
            return Some(resolved);
        }
    }

    let wanted = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(specifier)
        .to_ascii_lowercase();
    let matches = files
        .iter()
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|file| {
            file.path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.eq_ignore_ascii_case(&wanted))
                .unwrap_or(false)
        })
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}
