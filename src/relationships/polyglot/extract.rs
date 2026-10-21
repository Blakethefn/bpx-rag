#[path = "extractors/mod.rs"]
mod extractors;

use std::path::Path;

use super::super::languages::Grammar;
use super::super::metadata::ProjectMetadata;
use super::super::parsers::ParserCache;
use super::super::types::{RawReference, SourceFile};

pub(super) fn extract_references(
    grammar: Grammar,
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
    parsers: &mut ParserCache,
) -> Vec<RawReference> {
    let Some(tree) = parsers.parse(grammar, &source.content) else {
        return Vec::new();
    };
    let mut pending = vec![tree.root_node()];
    let mut references = Vec::new();
    while let Some(node) = pending.pop() {
        for (kind, specifier) in
            extractors::extract_specifiers(grammar, node, source, root, files, metadata)
        {
            references.push(RawReference {
                kind,
                specifier,
                line: node.start_position().row + 1,
                target: None,
            });
        }
        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    references
}
