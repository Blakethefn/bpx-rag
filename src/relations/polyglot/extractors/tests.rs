use std::path::{Path, PathBuf};

use super::super::super::super::languages::Grammar;
use super::super::super::super::metadata::ProjectMetadata;
use super::super::super::super::parsers::ParserCache;
use super::super::super::super::types::SourceFile;

fn references(grammar: Grammar, content: &str) -> Vec<(String, String)> {
    let mut parsers = ParserCache::default();
    let tree = parsers
        .parse(grammar, content)
        .unwrap_or_else(|| panic!("parse {grammar:?}"));
    let source = SourceFile {
        path: PathBuf::from("sample"),
        content: content.to_string(),
    };
    let metadata = ProjectMetadata::default();
    let mut pending = vec![tree.root_node()];
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        for (kind, specifier) in
            super::extract_specifiers(grammar, node, &source, Path::new("."), &[], &metadata)
        {
            found.push((kind.to_string(), specifier));
        }
        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    found.sort();
    found
}

fn listed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut found = pairs
        .iter()
        .map(|(kind, specifier)| (kind.to_string(), specifier.to_string()))
        .collect::<Vec<_>>();
    found.sort();
    found
}

mod modules;
mod scripting;
mod web;
