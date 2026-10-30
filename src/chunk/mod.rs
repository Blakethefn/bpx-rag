use std::path::Path;
use tree_sitter::Node;

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

#[derive(Debug)]
struct SyntaxBoundary {
    byte: usize,
    title: String,
}

fn chunk_syntax_tree(
    content: &str,
    root: Node<'_>,
    grammar: crate::relations::Grammar,
) -> Option<Vec<Chunk>> {
    let mut pending = vec![root];
    let mut boundaries = Vec::new();

    while let Some(node) = pending.pop() {
        let parent_is_wrapper = node
            .parent()
            .is_some_and(|parent| is_declaration_wrapper(parent.kind()));
        if is_declaration_boundary(node, grammar, content) && !parent_is_wrapper {
            let byte = include_leading_trivia(node, content);
            boundaries.push(SyntaxBoundary {
                byte,
                title: syntax_title(node, content),
            });
        }

        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }

    boundaries.sort_by(|left, right| left.byte.cmp(&right.byte));
    let mut unique_boundaries: Vec<SyntaxBoundary> = Vec::with_capacity(boundaries.len());
    for boundary in boundaries {
        if boundary.byte > content.len() || !content.is_char_boundary(boundary.byte) {
            continue;
        }
        if unique_boundaries
            .last()
            .is_some_and(|previous| previous.byte == boundary.byte)
        {
            continue;
        }
        unique_boundaries.push(boundary);
    }
    if unique_boundaries.is_empty() {
        return None;
    }

    let mut segments = vec![(0usize, "module".to_string())];
    let mut last_boundary = 0usize;
    for boundary in unique_boundaries {
        if boundary.byte == 0 {
            segments[0].1 = boundary.title;
            continue;
        }
        if boundary.byte.saturating_sub(last_boundary) < MIN_CHUNK_BYTES {
            continue;
        }
        segments.push((boundary.byte, boundary.title));
        last_boundary = boundary.byte;
    }

    let line_starts = line_start_offsets(content);
    let mut chunks = Vec::new();
    for (index, (start, title)) in segments.iter().enumerate() {
        let end = segments
            .get(index + 1)
            .map_or(content.len(), |(next_start, _)| *next_start);
        if end <= *start {
            continue;
        }
        let Some(section) = content.get(*start..end) else {
            return None;
        };
        let line_start = line_for_byte(&line_starts, *start);
        let line_end = line_for_byte(&line_starts, end - 1);
        if section.len() > MAX_CHUNK_BYTES {
            split_at_newlines_with_lines(title, section, &mut chunks, line_start);
        } else {
            chunks.push(Chunk {
                title: title.clone(),
                content: section.to_string(),
                line_start,
                line_end,
            });
        }
    }

    (!chunks.is_empty()).then_some(chunks)
}

fn is_declaration_wrapper(kind: &str) -> bool {
    matches!(
        kind,
        "decorated_definition" | "export_statement" | "export_declaration" | "template_declaration"
    )
}

fn is_declaration_boundary(
    node: Node<'_>,
    grammar: crate::relations::Grammar,
    content: &str,
) -> bool {
    let kind = node.kind();
    use crate::relations::Grammar;

    match grammar {
        Grammar::Rust => matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "impl_item"
                | "trait_item"
                | "mod_item"
                | "type_item"
                | "const_item"
                | "static_item"
                | "macro_definition"
        ),
        Grammar::JavaScript | Grammar::TypeScript | Grammar::Tsx => {
            is_declaration_wrapper(kind)
                || matches!(
                    kind,
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                )
                || (kind == "method_definition"
                    && node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "class_body"))
                || (matches!(kind, "lexical_declaration" | "variable_declaration")
                    && contains_function_value(node))
        }
        Grammar::C | Grammar::Cpp => {
            kind == "function_definition"
                || (matches!(
                    kind,
                    "struct_specifier" | "enum_specifier" | "class_specifier"
                ) && is_type_definition_context(node))
                || (grammar == Grammar::Cpp && kind == "namespace_definition")
        }
        Grammar::CSharp => matches!(
            kind,
            "namespace_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "constructor_declaration"
                | "property_declaration"
                | "delegate_declaration"
        ),
        Grammar::Go => matches!(
            kind,
            "function_declaration" | "method_declaration" | "type_declaration"
        ),
        Grammar::Java => matches!(
            kind,
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "method_declaration"
                | "constructor_declaration"
        ),
        Grammar::Python => matches!(
            kind,
            "decorated_definition" | "function_definition" | "class_definition"
        ),
        Grammar::Php => matches!(
            kind,
            "function_definition"
                | "class_declaration"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "property_declaration"
        ),
        Grammar::Ruby => matches!(kind, "class" | "module" | "method" | "singleton_method"),
        Grammar::Bash => kind == "function_definition",
        Grammar::Lua => kind == "function_declaration",
        Grammar::Dart => matches!(
            kind,
            "class_definition"
                | "enum_declaration"
                | "mixin_declaration"
                | "extension_declaration"
                | "function_declaration"
                | "method_signature"
        ),
        Grammar::Swift => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "enum_declaration"
                | "protocol_declaration"
                | "extension_declaration"
        ),
        Grammar::Kotlin => matches!(
            kind,
            "function_declaration" | "class_declaration" | "object_declaration"
        ),
        Grammar::Sql => kind == "statement",
        Grammar::Scala => matches!(
            kind,
            "class_definition"
                | "trait_definition"
                | "object_definition"
                | "enum_definition"
                | "function_definition"
                | "given_definition"
                | "extension_definition"
                | "package_object"
        ),
        Grammar::Haskell => matches!(
            kind,
            "declaration"
                | "function"
                | "data_type"
                | "newtype"
                | "type_synonym"
                | "class"
                | "instance"
                | "foreign_import"
        ),
        Grammar::Elixir => is_elixir_definition(node, content),
        Grammar::Erlang => matches!(kind, "function_clause" | "ssr_definition"),
        Grammar::R => kind == "function_definition",
        Grammar::Julia => matches!(
            kind,
            "function_definition"
                | "macro_definition"
                | "module_definition"
                | "struct_definition"
                | "abstract_definition"
                | "primitive_definition"
        ),
        Grammar::Perl => matches!(
            kind,
            "package_statement" | "function_definition" | "function_definition_without_sub"
        ),
        Grammar::PowerShell => {
            matches!(
                kind,
                "function_statement" | "class_statement" | "enum_statement"
            )
        }
        Grammar::Solidity => matches!(
            kind,
            "contract_declaration"
                | "interface_declaration"
                | "library_declaration"
                | "function_definition"
                | "constructor_definition"
                | "modifier_definition"
                | "struct_declaration"
                | "enum_declaration"
                | "event_definition"
                | "error_declaration"
                | "user_defined_type_definition"
        ),
        Grammar::Html => {
            matches!(kind, "element" | "script_element" | "style_element")
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "document")
        }
        Grammar::Css => {
            matches!(
                kind,
                "rule_set"
                    | "at_rule"
                    | "charset_statement"
                    | "import_statement"
                    | "keyframes_statement"
                    | "media_statement"
                    | "namespace_statement"
                    | "scope_statement"
                    | "supports_statement"
                    | "declaration"
            ) && node
                .parent()
                .is_some_and(|parent| parent.kind() == "stylesheet")
        }
        Grammar::Markdown => false,
    }
}

fn is_elixir_definition(node: Node<'_>, content: &str) -> bool {
    if node.kind() != "call" {
        return false;
    }
    node.child_by_field_name("target")
        .and_then(|target| target.utf8_text(content.as_bytes()).ok())
        .is_some_and(|target| {
            matches!(
                target.trim(),
                "defmodule"
                    | "defprotocol"
                    | "defimpl"
                    | "def"
                    | "defp"
                    | "defmacro"
                    | "defmacrop"
                    | "defguard"
                    | "defguardp"
            )
        })
}

fn is_type_definition_context(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if matches!(
            parent.kind(),
            "function_definition"
                | "compound_statement"
                | "parameter_declaration"
                | "parameter_list"
        ) {
            return false;
        }
        if matches!(
            parent.kind(),
            "type_definition"
                | "declaration"
                | "field_declaration"
                | "translation_unit"
                | "source_file"
        ) {
            return node.child_by_field_name("body").is_some()
                || (0..node.named_child_count() as u32).any(|index| {
                    node.named_child(index)
                        .is_some_and(|child| child.kind().ends_with("declaration_list"))
                });
        }
        ancestor = parent.parent();
    }
    false
}

fn contains_function_value(node: Node<'_>) -> bool {
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        if matches!(current.kind(), "arrow_function" | "function_expression") {
            return true;
        }
        for index in (0..current.named_child_count() as u32).rev() {
            if let Some(child) = current.named_child(index) {
                pending.push(child);
            }
        }
    }
    false
}

fn include_leading_trivia(node: Node<'_>, content: &str) -> usize {
    let mut start = node.start_byte();
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        if !is_leading_trivia(sibling.kind())
            || sibling.end_byte() > start
            || !starts_on_own_line(content, sibling.start_byte())
        {
            break;
        }
        let Some(gap) = content.get(sibling.end_byte()..start) else {
            break;
        };
        if !gap.chars().all(char::is_whitespace) {
            break;
        }
        start = sibling.start_byte();
        previous = sibling.prev_named_sibling();
    }
    start
}

fn starts_on_own_line(content: &str, byte: usize) -> bool {
    let Some(prefix) = content.get(..byte) else {
        return false;
    };
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    content
        .get(line_start..byte)
        .is_some_and(|indentation| indentation.chars().all(char::is_whitespace))
}

fn is_leading_trivia(kind: &str) -> bool {
    kind.contains("comment")
        || kind.contains("attribute")
        || matches!(kind, "decorator" | "shebang")
}

fn syntax_title(node: Node<'_>, content: &str) -> String {
    let name = declaration_name(node, content);

    let parent_name = node.parent().and_then(|mut parent| loop {
        if is_named_container(parent.kind()) {
            if let Some(name_node) = parent.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                    if !name.trim().is_empty() {
                        return Some(name.trim());
                    }
                }
            }
        }
        parent = parent.parent()?;
    });

    let title = match (parent_name, name) {
        (Some(parent), Some(name)) => format!("{parent}::{name}"),
        (None, Some(name)) => format!("{} {name}", node.kind().replace('_', " ")),
        (Some(parent), None) => format!("{parent}::{}", node.kind().replace('_', " ")),
        (None, None) => node.kind().replace('_', " "),
    };
    truncate_title(&title, 96)
}

fn declaration_name<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
            if !name.trim().is_empty() {
                return Some(name.trim());
            }
        }
    }

    let mut pending = Vec::new();
    for index in (0..node.named_child_count() as u32).rev() {
        if let Some(child) = node.named_child(index) {
            pending.push(child);
        }
    }
    while let Some(child) = pending.pop() {
        if let Some(name_node) = child.child_by_field_name("name") {
            if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                if !name.trim().is_empty() {
                    return Some(name.trim());
                }
            }
        }
        for index in (0..child.named_child_count() as u32).rev() {
            if let Some(grandchild) = child.named_child(index) {
                pending.push(grandchild);
            }
        }
    }
    None
}

fn is_named_container(kind: &str) -> bool {
    [
        "class",
        "struct",
        "enum",
        "trait",
        "impl",
        "interface",
        "namespace",
        "module",
        "extension",
        "protocol",
        "object",
        "record",
    ]
    .iter()
    .any(|token| kind.contains(token))
}

fn truncate_title(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn line_start_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(content.bytes().filter(|byte| *byte == b'\n').count() + 1);
    offsets.push(0);
    offsets.extend(
        content
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
    );
    offsets
}

fn line_for_byte(line_starts: &[usize], byte: usize) -> u32 {
    line_starts.partition_point(|start| *start <= byte).max(1) as u32
}

/// Check if a line is a structural boundary in code
fn is_structural_boundary(line: &str) -> bool {
    let trimmed = line.trim();

    // PHP / JS functions
    trimmed.starts_with("function ")
        || trimmed.starts_with("function(")
        || trimmed.starts_with("public function ")
        || trimmed.starts_with("private function ")
        || trimmed.starts_with("protected function ")
        || trimmed.starts_with("static function ")
        || trimmed.starts_with("async function ")
        // Classes, traits, interfaces
        || trimmed.starts_with("class ")
        || trimmed.starts_with("abstract class ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("interface ")
        // Python
        || trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        // Rust
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("pub struct ")
        || trimmed.starts_with("pub enum ")
        // HTML embedded sections
        || trimmed.starts_with("<script")
        || trimmed.starts_with("<style")
        || trimmed.starts_with("</script>")
        || trimmed.starts_with("</style>")
        // PHP tags
        || trimmed.starts_with("<?php")
        || trimmed == "?>"
        // CSS at-rules
        || trimmed.starts_with("@media")
        || trimmed.starts_with("@keyframes")
        // JS arrow/const functions (common pattern)
        || (trimmed.starts_with("const ") && (trimmed.contains("=> {") || trimmed.contains("= function")))
        || (trimmed.starts_with("let ") && (trimmed.contains("=> {") || trimmed.contains("= function")))
}

/// Chunk source code by structural boundaries
fn chunk_code(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_bytes: usize = 0;
    let mut current_title: Option<String> = None;
    let mut line_number: u32 = 0;
    let mut chunk_line_start: u32 = 1;

    for line in content.lines() {
        line_number += 1;
        let line_bytes = line.len() + 1; // +1 for newline

        if is_structural_boundary(line) && current_bytes >= MIN_CHUNK_BYTES {
            // Flush current buffer
            let body = current_lines.join("\n");
            let title = current_title
                .take()
                .unwrap_or_else(|| make_code_title(&current_lines));
            let chunk_line_end = line_number - 1;
            flush_chunk_with_lines(&mut chunks, &title, &body, chunk_line_start, chunk_line_end);
            current_lines.clear();
            current_bytes = 0;
            chunk_line_start = line_number;
        }

        // Set title from first structural boundary in this chunk
        if current_title.is_none() && is_structural_boundary(line) {
            let trimmed = line.trim();
            let label = if trimmed.len() > 60 {
                format!("{}...", &trimmed[..57])
            } else {
                trimmed.to_string()
            };
            current_title = Some(label);
        }

        current_lines.push(line);
        current_bytes += line_bytes;
    }

    // Flush remaining
    if !current_lines.is_empty() {
        let body = current_lines.join("\n");
        let title = current_title.unwrap_or_else(|| make_code_title(&current_lines));
        flush_chunk_with_lines(&mut chunks, &title, &body, chunk_line_start, line_number);
    }

    if chunks.is_empty() {
        chunks.push(Chunk {
            title: "code".to_string(),
            content: content.to_string(),
            line_start: 1,
            line_end: line_number.max(1),
        });
    }

    chunks
}

/// Generate a title from the first meaningful line of a code chunk
fn make_code_title(lines: &[&str]) -> String {
    for line in lines.iter().take(5) {
        let trimmed = line.trim();
        if !trimmed.is_empty() && trimmed.len() > 2 {
            let label = if trimmed.len() > 60 {
                format!("{}...", &trimmed[..57])
            } else {
                trimmed.to_string()
            };
            return label;
        }
    }
    "code-section".to_string()
}

/// Chunk markdown content by headings
fn chunk_markdown(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_title = String::from("(untitled)");
    let mut current_lines: Vec<&str> = Vec::new();
    let mut in_code_block = false;
    let mut line_number: u32 = 0;
    let mut chunk_line_start: u32 = 1;

    for line in content.lines() {
        line_number += 1;

        // Track code block boundaries
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            current_lines.push(line);
            continue;
        }

        // Split on headings (only outside code blocks)
        if !in_code_block && line.starts_with('#') {
            // Flush previous section
            if !current_lines.is_empty() {
                let body = current_lines.join("\n");
                let chunk_line_end = line_number - 1;
                flush_chunk_with_lines(
                    &mut chunks,
                    &current_title,
                    &body,
                    chunk_line_start,
                    chunk_line_end,
                );
                current_lines.clear();
            }
            current_title = line.trim_start_matches('#').trim().to_string();
            chunk_line_start = line_number;
            current_lines.push(line);
        } else {
            current_lines.push(line);
        }
    }

    // Flush final section
    if !current_lines.is_empty() {
        let body = current_lines.join("\n");
        flush_chunk_with_lines(
            &mut chunks,
            &current_title,
            &body,
            chunk_line_start,
            line_number,
        );
    }

    if chunks.is_empty() {
        chunks.push(Chunk {
            title: "(untitled)".to_string(),
            content: content.to_string(),
            line_start: 1,
            line_end: line_number.max(1),
        });
    }

    chunks
}

/// Chunk plain text by paragraphs
fn chunk_plain_text(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let paragraphs: Vec<&str> = content.split("\n\n").collect();

    let mut current = String::new();
    let mut idx = 0u32;
    // Track cumulative line offset for line number attribution
    let mut cumulative_lines: u32 = 0;
    let mut chunk_line_start: u32 = 1;

    for para in &paragraphs {
        let para_trimmed = para.trim();
        let para_lines = para.lines().count() as u32;

        if para_trimmed.is_empty() {
            continue;
        }

        if current.len() + para_trimmed.len() + 2 > MAX_CHUNK_BYTES && !current.is_empty() {
            idx += 1;
            let chunk_line_end = cumulative_lines.saturating_sub(1).max(chunk_line_start);
            let taken = std::mem::take(&mut current);
            chunks.push(Chunk {
                title: format!("section-{idx}"),
                content: taken,
                line_start: chunk_line_start,
                line_end: chunk_line_end,
            });
            chunk_line_start = cumulative_lines + 1;
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para_trimmed);
        // A two-newline paragraph separator advances the next paragraph by
        // two line numbers: one separator line plus the paragraph's lines.
        cumulative_lines += para_lines + 1;
    }

    if !current.is_empty() {
        // If the accumulated content is oversized (no \n\n splits helped), use newline fallback
        if current.len() > MAX_CHUNK_BYTES {
            split_at_newlines_with_lines(
                &format!("section-{}", idx + 1),
                &current,
                &mut chunks,
                chunk_line_start,
            );
        } else {
            idx += 1;
            let total_lines = content.lines().count() as u32;
            chunks.push(Chunk {
                title: format!("section-{idx}"),
                content: current,
                line_start: chunk_line_start,
                line_end: total_lines.max(chunk_line_start),
            });
        }
    }

    chunks
}

/// Flush accumulated content as one or more chunks (splitting if oversized), with line info
fn flush_chunk_with_lines(
    chunks: &mut Vec<Chunk>,
    title: &str,
    content: &str,
    line_start: u32,
    line_end: u32,
) {
    if content.len() <= MAX_CHUNK_BYTES {
        chunks.push(Chunk {
            title: title.to_string(),
            content: content.to_string(),
            line_start,
            line_end,
        });
        return;
    }

    // Try splitting at paragraph boundaries first
    let paragraphs: Vec<&str> = content.split("\n\n").collect();

    // If there's only one "paragraph" (no \n\n), go straight to newline splitting
    if paragraphs.len() <= 1 {
        split_at_newlines_with_lines(title, content, chunks, line_start);
        return;
    }

    let mut current = String::new();
    let mut part = 0u32;
    let mut paragraph_line_start = line_start;
    let mut part_line_start = line_start;

    for (paragraph_index, para) in paragraphs.iter().enumerate() {
        if current.len() + para.len() + 2 > MAX_CHUNK_BYTES && !current.is_empty() {
            part += 1;
            let part_line_end = paragraph_line_start.saturating_sub(1).max(part_line_start);
            let taken = std::mem::take(&mut current);
            if taken.len() > MAX_CHUNK_BYTES {
                split_at_newlines_with_lines(
                    &format!("{title} (part {part})"),
                    &taken,
                    chunks,
                    part_line_start,
                );
            } else {
                chunks.push(Chunk {
                    title: format!("{title} (part {part})"),
                    content: taken,
                    line_start: part_line_start,
                    line_end: part_line_end,
                });
            }
            part_line_start = paragraph_line_start;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
        if paragraph_index + 1 < paragraphs.len() {
            paragraph_line_start += para.matches('\n').count() as u32 + 2;
        }
    }

    if !current.is_empty() {
        part += 1;
        let part_title = if part > 1 {
            format!("{title} (part {part})")
        } else {
            title.to_string()
        };
        if current.len() > MAX_CHUNK_BYTES {
            split_at_newlines_with_lines(&part_title, &current, chunks, part_line_start);
        } else {
            chunks.push(Chunk {
                title: part_title,
                content: current,
                line_start: part_line_start,
                line_end,
            });
        }
    }
}

/// Hard split at single newline boundaries with line number tracking
fn split_at_newlines_with_lines(
    title: &str,
    content: &str,
    chunks: &mut Vec<Chunk>,
    base_line_start: u32,
) {
    let mut current = String::new();
    let mut part = 0u32;
    let mut line_offset: u32 = 0;
    let mut part_line_start = base_line_start;

    for line in content.lines() {
        line_offset += 1;
        // +1 for the newline we'll add
        if current.len() + line.len() + 1 > MAX_CHUNK_BYTES && !current.is_empty() {
            part += 1;
            // line_offset is 1-based and has already advanced past the overflow line,
            // so subtract 2 to get the last line actually included in the current chunk.
            let part_line_end = (base_line_start + line_offset)
                .saturating_sub(2)
                .max(part_line_start);
            let taken = std::mem::take(&mut current);
            chunks.push(Chunk {
                title: format!("{title} (part {part})"),
                content: taken,
                line_start: part_line_start,
                line_end: part_line_end,
            });
            part_line_start = base_line_start + line_offset - 1;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);

        // Handle single lines longer than MAX_CHUNK_BYTES — hard byte split
        while current.len() > MAX_CHUNK_BYTES {
            // Find a char boundary at or before MAX_CHUNK_BYTES
            let mut split_at = MAX_CHUNK_BYTES;
            while split_at > 0 && !current.is_char_boundary(split_at) {
                split_at -= 1;
            }
            if split_at == 0 {
                split_at = current.len().min(MAX_CHUNK_BYTES);
            }
            part += 1;
            let piece: String = current.drain(..split_at).collect();
            chunks.push(Chunk {
                title: format!("{title} (part {part})"),
                content: piece,
                line_start: part_line_start,
                line_end: base_line_start + line_offset - 1,
            });
            part_line_start = base_line_start + line_offset - 1;
        }
    }

    if !current.is_empty() {
        part += 1;
        let part_line_end = base_line_start + line_offset - 1;
        chunks.push(Chunk {
            title: if part > 1 {
                format!("{title} (part {part})")
            } else {
                title.to_string()
            },
            content: current,
            line_start: part_line_start,
            line_end: part_line_end,
        });
    }
}

#[cfg(test)]
mod tests;
