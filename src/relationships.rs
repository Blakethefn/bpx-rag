use std::path::Path;

mod format;
mod languages;
mod markdown;
mod metadata;
mod parsers;
mod paths;
mod polyglot;
mod rust;
mod scan;
mod script;
mod types;

pub(crate) use languages::Grammar;
use tree_sitter::Tree;

pub use format::format_analysis;
pub use scan::analyze_file;

/// Parse a supported source file for the shared indexing and relationship paths.
pub(crate) fn parse_tree_for_path(path: &Path, content: &str) -> Option<(Grammar, Tree)> {
    let grammar = languages::grammar_for_path(path)?;
    if grammar == Grammar::Markdown {
        return None;
    }
    let tree = parsers::ParserCache::default().parse(grammar, content)?;
    Some((grammar, tree))
}

/// Whether the path has a bundled source grammar that can version its chunks.
pub(crate) fn supports_tree_sitter_chunking(path: &Path) -> bool {
    languages::grammar_for_path(path).is_some_and(|grammar| grammar != Grammar::Markdown)
}

#[cfg(test)]
mod tests;
