use super::*;

#[test]
fn test_chunk_markdown_splits_on_headings() {
    let md = "# Title\nSome text\n## Section\nMore text";
    let chunks = chunk_content(md);
    assert!(chunks.len() >= 2);
    assert_eq!(chunks[0].title, "Title");
    assert_eq!(chunks[1].title, "Section");
}

#[test]
fn test_chunk_plain_text() {
    let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
    let chunks = chunk_content(text);
    assert!(!chunks.is_empty());
}

#[test]
fn test_code_blocks_stay_intact() {
    let md = "# Example\n```rust\nfn main() {\n    // code\n}\n```\nSome text after.";
    let chunks = chunk_content(md);
    // The code block should be in the same chunk as its heading
    assert!(chunks[0].content.contains("fn main()"));
}

#[test]
fn test_structural_boundary_detection() {
    assert!(is_structural_boundary("function getUser($id) {"));
    assert!(is_structural_boundary("public function handle() {"));
    assert!(is_structural_boundary("class UserController {"));
    assert!(is_structural_boundary("<script>"));
    assert!(is_structural_boundary("<style>"));
    assert!(is_structural_boundary("<?php"));
    assert!(is_structural_boundary("def process(data):"));
    assert!(is_structural_boundary("pub fn new() -> Self {"));
    assert!(is_structural_boundary("  const handler = () => {"));
    assert!(!is_structural_boundary("    $x = 1;"));
    assert!(!is_structural_boundary("    // just a comment"));
}

#[test]
fn test_code_chunking_produces_small_chunks() {
    // Build a PHP-like file with multiple functions
    let mut content = String::from("<?php\nrequire 'config.php';\n\n");
    for i in 0..20 {
        content.push_str(&format!(
            "function func_{i}() {{\n    $x = {i};\n    return $x * 2;\n}}\n\n"
        ));
    }
    content.push_str("?>");

    let chunks = chunk_content(&content);

    // Should produce multiple chunks, not 1-3
    assert!(
        chunks.len() >= 5,
        "Expected >=5 chunks, got {}",
        chunks.len()
    );

    // Every chunk must respect MAX_CHUNK_BYTES
    for chunk in &chunks {
        assert!(
            chunk.content.len() <= MAX_CHUNK_BYTES,
            "Chunk '{}' is {} bytes, exceeds {}",
            chunk.title,
            chunk.content.len(),
            MAX_CHUNK_BYTES
        );
    }
}

#[test]
fn test_no_chunk_exceeds_max_bytes_dense_content() {
    // Dense content with no blank lines — simulates minified-ish code
    let mut content = String::new();
    for i in 0..500 {
        content.push_str(&format!("var x_{i} = {i};\n"));
    }

    let chunks = chunk_content(&content);

    for chunk in &chunks {
        assert!(
            chunk.content.len() <= MAX_CHUNK_BYTES,
            "Chunk '{}' is {} bytes, exceeds {}",
            chunk.title,
            chunk.content.len(),
            MAX_CHUNK_BYTES
        );
    }
}

#[test]
fn test_no_chunk_exceeds_max_bytes_no_newlines() {
    // Single long line with no newlines at all
    let content = "x".repeat(10000);
    let chunks = chunk_content(&content);

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(
            chunk.content.len() <= MAX_CHUNK_BYTES,
            "Chunk '{}' is {} bytes, exceeds {}",
            chunk.title,
            chunk.content.len(),
            MAX_CHUNK_BYTES
        );
    }
}

#[test]
fn test_code_chunks_have_line_numbers() {
    // Build a Rust-like file with multiple functions
    let content = "fn first() {\n    let x = 1;\n    let y = 2;\n    let z = x + y;\n}\n\nfn second() {\n    let a = 10;\n    let b = 20;\n    let c = a + b;\n}\n\nfn third() {\n    let p = 100;\n    let q = 200;\n    let r = p + q;\n}";
    // Force code detection by making it look like code
    let chunks = chunk_code(content);
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(
            chunk.line_start > 0,
            "line_start should be > 0 for code chunks"
        );
        assert!(
            chunk.line_end >= chunk.line_start,
            "line_end should be >= line_start"
        );
    }
    // First chunk should start at line 1
    assert_eq!(
        chunks[0].line_start, 1,
        "First chunk should start at line 1"
    );
}

#[test]
fn test_markdown_chunks_have_line_numbers() {
    let md = "# Section One\nFirst section content.\nMore content here.\n## Section Two\nSecond section content.\nEven more content.\n### Subsection\nSubsection content.";
    let chunks = chunk_markdown(md);
    assert!(
        chunks.len() >= 2,
        "Expected at least 2 chunks from markdown with headings"
    );
    // First chunk should start at line 1
    assert_eq!(
        chunks[0].line_start, 1,
        "First markdown chunk should start at line 1"
    );
    // Each chunk should have valid line numbers
    for chunk in &chunks {
        assert!(chunk.line_start > 0, "line_start should be > 0");
        assert!(
            chunk.line_end >= chunk.line_start,
            "line_end should be >= line_start"
        );
    }
    // Chunks should be in ascending order
    for i in 1..chunks.len() {
        assert!(
            chunks[i].line_start > chunks[i - 1].line_start,
            "Chunk line_start values should be strictly increasing"
        );
    }
}

#[test]
fn test_plain_text_chunks_have_line_numbers() {
    let text = "First paragraph with some text.\nMore lines here.\n\nSecond paragraph with text.\nAdditional lines.\n\nThird paragraph.";
    let chunks = chunk_plain_text(text);
    assert!(!chunks.is_empty());
    // First chunk should start at line 1
    assert_eq!(
        chunks[0].line_start, 1,
        "First plain text chunk should start at line 1"
    );
    for chunk in &chunks {
        assert!(chunk.line_start > 0, "line_start should be > 0");
        assert!(
            chunk.line_end >= chunk.line_start,
            "line_end should be >= line_start"
        );
    }
}

#[test]
fn plain_text_chunk_line_numbers_account_for_blank_line_separator() {
    let first = format!("{}\n{}", "a".repeat(1500), "b".repeat(1500));
    let second = format!("{}\n{}", "c".repeat(1500), "d".repeat(1500));
    let chunks = chunk_plain_text(&format!("{first}\n\n{second}"));

    assert_eq!(chunks.len(), 2);
    assert_eq!((chunks[0].line_start, chunks[0].line_end), (1, 2));
    assert_eq!((chunks[1].line_start, chunks[1].line_end), (4, 5));
}

#[test]
fn test_line_numbers_cover_full_content() {
    let content = "# Introduction\nLine 2.\nLine 3.\n## Main Section\nLine 5.\nLine 6.\nLine 7.\n## Conclusion\nLine 9.\nLine 10.";
    let total_lines = content.lines().count() as u32;
    let chunks = chunk_markdown(content);

    assert!(!chunks.is_empty());
    // First chunk starts at line 1
    assert_eq!(chunks[0].line_start, 1, "Coverage should start at line 1");
    // Last chunk ends at or near total line count
    let last = chunks.last().unwrap();
    assert!(
        last.line_end >= total_lines,
        "Last chunk line_end ({}) should cover total lines ({})",
        last.line_end,
        total_lines
    );
}

#[test]
fn oversized_paragraph_chunks_keep_ordered_source_line_ranges() {
    let paragraph = "x".repeat(2500);
    let content = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
    let mut chunks = Vec::new();

    flush_chunk_with_lines(&mut chunks, "large", &content, 100, 104);

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].line_start, 100);
    assert_eq!(chunks[0].line_end, 101);
    assert_eq!(chunks[1].line_start, 102);
    assert_eq!(chunks[1].line_end, 103);
    assert_eq!(chunks[2].line_start, 104);
    assert_eq!(chunks[2].line_end, 104);
    assert!(chunks
        .iter()
        .all(|chunk| chunk.line_start <= chunk.line_end));
}

#[test]
fn oversized_paragraph_chunks_preserve_extra_blank_lines_in_ranges() {
    let paragraph = "x".repeat(2500);
    let content = format!("{paragraph}\n\n\n{paragraph}\n\n\n{paragraph}");
    let mut chunks = Vec::new();

    flush_chunk_with_lines(&mut chunks, "large", &content, 100, 106);

    assert_eq!(chunks.len(), 3);
    assert_eq!((chunks[0].line_start, chunks[0].line_end), (100, 101));
    assert_eq!((chunks[1].line_start, chunks[1].line_end), (102, 104));
    assert_eq!((chunks[2].line_start, chunks[2].line_end), (105, 106));
}
