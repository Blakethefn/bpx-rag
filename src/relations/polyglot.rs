mod extract;
mod resolve;

use std::path::Path;

use self::resolve::resolve_specifier;
use super::languages::Grammar;
use super::metadata::ProjectMetadata;
use super::parsers::ParserCache;
use super::types::{RawReference, SourceFile};

pub(super) fn polyglot_references(
    grammar: Grammar,
    source: &SourceFile,
    root: &Path,
    files: &[SourceFile],
    metadata: &ProjectMetadata,
    parsers: &mut ParserCache,
) -> Vec<RawReference> {
    extract::extract_references(grammar, source, root, files, metadata, parsers)
        .into_iter()
        .flat_map(|reference| {
            let targets = resolve_specifier(
                grammar,
                &source.path,
                root,
                &reference.specifier,
                files,
                metadata,
            );
            if targets.is_empty() {
                vec![reference]
            } else {
                let kind = reference.kind;
                let specifier = reference.specifier;
                let line = reference.line;
                targets
                    .into_iter()
                    .map(|target| RawReference {
                        kind,
                        specifier: specifier.clone(),
                        line,
                        target: Some(target),
                    })
                    .collect()
            }
        })
        .collect()
}
