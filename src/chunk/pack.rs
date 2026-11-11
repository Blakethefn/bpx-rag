use super::{Chunk, MAX_CHUNK_BYTES};

/// Flush accumulated content as one or more chunks (splitting if oversized), with line info
pub(in crate::chunk) fn flush_chunk_with_lines(
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
pub(in crate::chunk) fn split_at_newlines_with_lines(
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
