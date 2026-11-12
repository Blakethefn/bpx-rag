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

/// Maximum chunk size in bytes
const MAX_CHUNK_BYTES: usize = 4096;

/// Minimum chunk size — avoid splitting too aggressively
const MIN_CHUNK_BYTES: usize = 256;

/// Avoid building syntax trees for generated or unusually large source files.
const MAX_TREE_SITTER_FILE_BYTES: usize = 2 * 1024 * 1024;

/// Bump when the structural chunking policy changes for supported source files.
pub(crate) const TREE_SITTER_CHUNKER_VERSION: i64 = 2;

/// A chunk of content ready for indexing
#[derive(Debug)]
pub struct Chunk {
    pub title: String,
    pub content: String,
    /// 1-based line number where this chunk starts (0 if unknown)
    pub line_start: u32,
    /// 1-based line number where this chunk ends, inclusive (0 if unknown)
    pub line_end: u32,
}

/// Chunks produced for a path, including whether Tree-sitter supplied boundaries.
pub struct ChunkingResult {
    pub chunks: Vec<Chunk>,
    pub used_tree_sitter: bool,
    /// Named declarations from the same parse. Empty unless the walk completed and found none,
    /// or the file was unsupported, a parse error, or over the Tree-sitter cap.
    pub symbols: Vec<Symbol>,
    /// `complete`, `unsupported`, `parse_error`, or `too_large`.
    pub(crate) symbol_status: &'static str,
    /// Occurrences in this file. Empty for `parse_error` and `too_large`.
    pub(crate) references: Vec<Reference>,
    /// Program, test, and manifest observations made from this file's bytes.
    pub(crate) entries: Vec<EntryObservation>,
}

/// Split content into indexable chunks
///
/// Strategy:
/// - Markdown: split on headings, keep code blocks intact
/// - Code: split on structural boundaries (functions, classes, tags)
/// - Plain text: split on double newlines (paragraphs)
/// - All paths enforce MAX_CHUNK_BYTES via newline fallback
pub fn chunk_content(content: &str) -> Vec<Chunk> {
    match classify_content(content) {
        ContentType::Markdown => chunk_markdown(content),
        ContentType::Code => chunk_code(content),
        ContentType::PlainText => chunk_plain_text(content),
    }
}

/// Chunk a source file using its bundled grammar when parsing succeeds.
/// Markdown, unknown languages, malformed source, and large files keep the
/// existing content-based chunker as a fallback.
///
/// Symbol status distinguishes a finished walk that declared nothing from a file that was not
/// analyzed. References and manifest entries come from the same bytes as the symbols.
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

/// The stored per-file chunking version allows sync to rebuild stale chunks.
pub(crate) fn chunker_version_for_path(path: &Path) -> i64 {
    if crate::relations::supports_tree_sitter_chunking(path) {
        TREE_SITTER_CHUNKER_VERSION
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
