use super::{Chunk, MAX_CHUNK_BYTES};

pub(in crate::chunk) use super::pack::{flush_chunk_with_lines, split_at_newlines_with_lines};

pub(super) fn chunk_markdown(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_title = String::from("(untitled)");
    let mut current_lines: Vec<&str> = Vec::new();
    let mut in_code_block = false;
    let mut line_number: u32 = 0;
    let mut chunk_line_start: u32 = 1;

    for line in content.lines() {
        line_number += 1;

        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            current_lines.push(line);
            continue;
        }

        if !in_code_block && line.starts_with('#') {
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

pub(super) fn chunk_plain_text(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let paragraphs: Vec<&str> = content.split("\n\n").collect();

    let mut current = String::new();
    let mut idx = 0u32;
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
        cumulative_lines += para_lines + 1;
    }

    if !current.is_empty() {
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
