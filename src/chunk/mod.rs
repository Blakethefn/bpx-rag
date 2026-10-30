use std::path::Path;

mod syntax;
mod code;
mod text;
mod lines;
use syntax::*;
use code::*;
use text::*;
use lines::*;

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
}

/// Result of single-pass content classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentType {
    Markdown,
    Code,
    PlainText,
}

/// Classify content in a single pass over all lines.
///
/// Tracks markdown indicators and code indicators simultaneously,
/// then applies markdown-first priority: if markdown passes, return
/// Markdown even if code would also match.
fn classify_content(content: &str) -> ContentType {
    // Quick structural checks for code (these inspect the whole string, not per-line)
    let has_php_tag = content.contains("<?php") || content.contains("<?=");
    let has_script_pair = content.contains("<script") && content.contains("</script>");
    let has_style_pair = content.contains("<style") && content.contains("</style>");

    let mut md_score: u32 = 0;
    let mut md_code_counter: u32 = 0; // code counter-indicators for markdown rejection
    let mut line_count: u32 = 0;

    let mut non_empty_lines: u32 = 0;
    let mut code_endings: u32 = 0;
    let mut has_func = false;

    for line in content.lines() {
        line_count += 1;
        let trimmed = line.trim();

        // --- Markdown indicators ---
        if trimmed.starts_with("# ")
            || trimmed.starts_with("## ")
            || trimmed.starts_with("### ")
            || trimmed.starts_with("#### ")
        {
            md_score += 1;
        }
        if trimmed.starts_with("```") {
            md_score += 1;
        }
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if !trimmed.contains('{') && !trimmed.contains(';') {
                md_score += 1;
            }
        }

        // --- Code counter-indicators (for markdown rejection) ---
        if trimmed.contains("<?php")
            || trimmed.starts_with("<script")
            || trimmed.starts_with("<style")
            || trimmed.starts_with("<html")
            || trimmed.starts_with("<!DOCTYPE")
            || trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("package ")
            || trimmed.starts_with("use ")
        {
            md_code_counter += 1;
        }
        if trimmed.ends_with(';') || trimmed.ends_with('{') {
            md_code_counter += 1;
        }

        // --- Code detection indicators ---
        if !trimmed.is_empty() {
            non_empty_lines += 1;
            if trimmed.ends_with(';') || trimmed.ends_with('{') || trimmed.ends_with('}') {
                code_endings += 1;
            }
            if !has_func {
                has_func = trimmed.starts_with("function ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("pub fn ")
                    || trimmed.contains("function(")
                    || trimmed.contains("function (")
                    || trimmed.starts_with("class ");
            }
        }
    }

    // --- Markdown check (priority) ---
    if line_count > 0 && md_score > 0 {
        let md_passes =
            (md_score as f64 / line_count as f64) > 0.02 && !(md_code_counter > md_score * 2);
        if md_passes {
            return ContentType::Markdown;
        }
    }

    // --- Code check ---
    if has_php_tag || has_script_pair || has_style_pair {
        return ContentType::Code;
    }
    if non_empty_lines > 0 {
        let code_ratio = code_endings as f64 / non_empty_lines as f64;
        if code_ratio > 0.4 || (has_func && code_endings > 5) {
            return ContentType::Code;
        }
    }

    ContentType::PlainText
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
pub fn chunk_content_for_path(path: &Path, content: &str) -> ChunkingResult {
    if content.len() <= MAX_TREE_SITTER_FILE_BYTES {
        if let Some((grammar, tree)) = crate::relations::parse_tree_for_path(path, content) {
            let root = tree.root_node();
            if !root.has_error() {
                if let Some(chunks) = chunk_syntax_tree(content, root, grammar) {
                    return ChunkingResult {
                        chunks,
                        used_tree_sitter: true,
                    };
                }
            }
        }
    }

    ChunkingResult {
        chunks: chunk_content(content),
        used_tree_sitter: false,
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
