use std::path::Path;

mod classify;
mod code;
pub(crate) mod evidence;
mod lines;
mod pack;
mod symbols;
mod syntax;
mod text;
use classify::{classify_content, ContentType};
use code::*;
use lines::*;
use syntax::*;
use text::*;

pub(crate) use evidence::{EntryObservation, Reference};
pub use symbols::Symbol;

const MAX_CHUNK_BYTES: usize = 4096;

const MIN_CHUNK_BYTES: usize = 256;

const MAX_TREE_SITTER_FILE_BYTES: usize = 2 * 1024 * 1024;

pub(crate) const TREE_SITTER_CHUNKER_VERSION: i64 = 2;

#[derive(Debug)]
pub struct Chunk {
    pub title: String,
    pub content: String,
    pub line_start: u32,
    pub line_end: u32,
}

pub struct ChunkingResult {
    pub chunks: Vec<Chunk>,
    pub used_tree_sitter: bool,
    pub symbols: Vec<Symbol>,
    pub(crate) symbol_status: &'static str,
    pub(crate) references: Vec<Reference>,
    pub(crate) entries: Vec<EntryObservation>,
}

pub fn chunk_content(content: &str) -> Vec<Chunk> {
    match classify_content(content) {
        ContentType::Markdown => chunk_markdown(content),
        ContentType::Code => chunk_code(content),
        ContentType::PlainText => chunk_plain_text(content),
    }
}

pub fn chunk_content_for_path(path: &Path, content: &str) -> ChunkingResult {
    let supported = crate::relations::supports_tree_sitter_chunking(path);
    if supported && content.len() > MAX_TREE_SITTER_FILE_BYTES {
        return fallback(content, evidence::STATUS_TOO_LARGE, Vec::new());
    }
    if let Some((grammar, tree)) = crate::relations::parse_tree_for_path(path, content) {
        let root = tree.root_node();
        if !root.has_error() {
            let symbols = symbols::symbols_from_tree(content, root, grammar);
            let (references, entries) =
                evidence::references_and_entries(path, content, root, grammar);
            if let Some(chunks) = chunk_syntax_tree(content, root, grammar) {
                return ChunkingResult {
                    chunks,
                    used_tree_sitter: true,
                    symbols,
                    symbol_status: evidence::STATUS_COMPLETE,
                    references,
                    entries,
                };
            }
            return ChunkingResult {
                chunks: chunk_content(content),
                used_tree_sitter: false,
                symbols,
                symbol_status: evidence::STATUS_COMPLETE,
                references,
                entries,
            };
        }
        return fallback(content, evidence::STATUS_PARSE_ERROR, Vec::new());
    }
    let status = if supported {
        evidence::STATUS_PARSE_ERROR
    } else {
        evidence::STATUS_UNSUPPORTED
    };
    let entries = if status == evidence::STATUS_UNSUPPORTED {
        evidence::manifest_entries(path, content)
    } else {
        Vec::new()
    };
    fallback(content, status, entries)
}

fn fallback(
    content: &str,
    symbol_status: &'static str,
    entries: Vec<EntryObservation>,
) -> ChunkingResult {
    ChunkingResult {
        chunks: chunk_content(content),
        used_tree_sitter: false,
        symbols: Vec::new(),
        symbol_status,
        references: Vec::new(),
        entries,
    }
}

pub(crate) fn chunker_version_for_path(path: &Path) -> i64 {
    if crate::relations::supports_tree_sitter_chunking(path) {
        TREE_SITTER_CHUNKER_VERSION
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
