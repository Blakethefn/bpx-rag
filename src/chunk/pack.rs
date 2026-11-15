use super::{Chunk, MAX_CHUNK_BYTES};

mod split;

pub(in crate::chunk) use split::split_at_newlines_with_lines;

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

    let paragraphs: Vec<&str> = content.split("\n\n").collect();

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
