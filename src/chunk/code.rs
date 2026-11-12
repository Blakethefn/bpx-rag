use super::*;

pub(super) fn is_structural_boundary(line: &str) -> bool {
    let trimmed = line.trim();

    trimmed.starts_with("function ")
        || trimmed.starts_with("function(")
        || trimmed.starts_with("public function ")
        || trimmed.starts_with("private function ")
        || trimmed.starts_with("protected function ")
        || trimmed.starts_with("static function ")
        || trimmed.starts_with("async function ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("abstract class ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("interface ")
        || trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("pub struct ")
        || trimmed.starts_with("pub enum ")
        || trimmed.starts_with("<script")
        || trimmed.starts_with("<style")
        || trimmed.starts_with("</script>")
        || trimmed.starts_with("</style>")
        || trimmed.starts_with("<?php")
        || trimmed == "?>"
        || trimmed.starts_with("@media")
        || trimmed.starts_with("@keyframes")
        || (trimmed.starts_with("const ")
            && (trimmed.contains("=> {") || trimmed.contains("= function")))
        || (trimmed.starts_with("let ")
            && (trimmed.contains("=> {") || trimmed.contains("= function")))
}

pub(super) fn chunk_code(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_bytes: usize = 0;
    let mut current_title: Option<String> = None;
    let mut line_number: u32 = 0;
    let mut chunk_line_start: u32 = 1;

    for line in content.lines() {
        line_number += 1;
        let line_bytes = line.len() + 1;

        if is_structural_boundary(line) && current_bytes >= MIN_CHUNK_BYTES {
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

pub(super) fn make_code_title(lines: &[&str]) -> String {
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
