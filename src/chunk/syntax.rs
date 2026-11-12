use super::*;
use tree_sitter::Node;

#[derive(Debug)]
pub(super) struct SyntaxBoundary {
    pub(super) byte: usize,
    pub(super) title: String,
}

pub(super) fn declaration_nodes<'t>(
    root: Node<'t>,
    content: &str,
    grammar: crate::relations::Grammar,
) -> Vec<Node<'t>> {
    let mut pending = vec![root];
    let mut nodes = Vec::new();

    while let Some(node) = pending.pop() {
        let parent_is_wrapper = node
            .parent()
            .is_some_and(|parent| is_declaration_wrapper(parent.kind()));
        if is_declaration_boundary(node, grammar, content) && !parent_is_wrapper {
            nodes.push(node);
        }

        for index in (0..node.named_child_count() as u32).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    nodes
}

pub(super) fn chunk_syntax_tree(
    content: &str,
    root: Node<'_>,
    grammar: crate::relations::Grammar,
) -> Option<Vec<Chunk>> {
    let mut boundaries: Vec<SyntaxBoundary> = declaration_nodes(root, content, grammar)
        .into_iter()
        .map(|node| SyntaxBoundary {
            byte: include_leading_trivia(node, content),
            title: syntax_title(node, content),
        })
        .collect();

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
        let section = content.get(*start..end)?;
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

mod boundary;
mod title;

use boundary::is_declaration_boundary;
pub(super) use boundary::is_declaration_wrapper;
pub(super) use title::is_named_container;
use title::syntax_title;

pub(super) fn include_leading_trivia(node: Node<'_>, content: &str) -> usize {
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

pub(super) fn starts_on_own_line(content: &str, byte: usize) -> bool {
    let Some(prefix) = content.get(..byte) else {
        return false;
    };
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    content
        .get(line_start..byte)
        .is_some_and(|indentation| indentation.chars().all(char::is_whitespace))
}

pub(super) fn is_leading_trivia(kind: &str) -> bool {
    kind.contains("comment")
        || kind.contains("attribute")
        || matches!(kind, "decorator" | "shebang")
}
