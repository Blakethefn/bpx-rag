use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use super::super::languages::{grammar_for_path, Grammar};
use super::super::markdown::markdown_references;
use super::super::metadata::ProjectMetadata;
use super::super::parsers::ParserCache;
use super::super::paths::{display_relative, is_supported};
use super::super::polyglot::polyglot_references;
use super::super::rust::rust_references;
use super::super::script::script_references;
use super::super::types::{FileRelationship, RawReference, SourceFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

pub fn project_edges(root: &Path, paths: &[PathBuf]) -> Result<Vec<ProjectEdge>> {
    let mut files = Vec::new();
    for path in paths {
        if !is_supported(path) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path) {
            files.push(SourceFile {
                path: path.clone(),
                content,
            });
        }
    }

    let metadata = ProjectMetadata::read(root);
    let mut parsers = ParserCache::default();
    let mut edges = Vec::new();
    for source in &files {
        let from = display_relative(&source.path, root)?;
        for reference in extract_references(source, root, &files, &metadata, &mut parsers) {
            let Some(target) = reference.target.as_deref() else {
                continue;
            };
            let Ok(to) = display_relative(target, root) else {
                continue;
            };
            if to != from {
                edges.push(ProjectEdge {
                    from: from.clone(),
                    to,
                    kind: reference.kind.to_string(),
                });
            }
        }
    }
    Ok(edges)
}

pub(super) fn relationships(
    target: &Path,
    root: &Path,
    files: &[SourceFile],
) -> Result<(Vec<FileRelationship>, Vec<FileRelationship>)> {
    let mut outgoing = Vec::new();
    let mut incoming = Vec::new();
    let metadata = ProjectMetadata::read(root);
    let mut parsers = ParserCache::default();

    for source in files {
        for reference in extract_references(source, root, files, &metadata, &mut parsers) {
            let relation = FileRelationship {
                from: display_relative(&source.path, root)?,
                to: reference
                    .target
                    .as_ref()
                    .and_then(|path| display_relative(path, root).ok()),
                kind: reference.kind.to_string(),
                specifier: reference.specifier,
                line: reference.line,
                direction: if source.path == target {
                    "outgoing".to_string()
                } else {
                    "incoming".to_string()
                },
            };

            if source.path == target {
                outgoing.push(relation);
            } else if reference.target.as_deref() == Some(target) {
                incoming.push(relation);
            }
        }
    }

    sort_relationships(&mut outgoing);
    sort_relationships(&mut incoming);
    Ok((outgoing, incoming))
}

fn sort_relationships(relationships: &mut [FileRelationship]) {
    relationships.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then(left.line.cmp(&right.line))
            .then(left.specifier.cmp(&right.specifier))
    });
}

fn extract_references(
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
    parsers: &mut ParserCache,
) -> Vec<RawReference> {
    match grammar_for_path(&source.path) {
        Some(Grammar::Markdown) => markdown_references(source, root, files),
        Some(Grammar::Rust) => rust_references(source, root, parsers),
        Some(Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx) => {
            script_references(source, root, parsers)
        }
        Some(grammar) => polyglot_references(grammar, source, root, files, metadata, parsers),
        None => Vec::new(),
    }
}
