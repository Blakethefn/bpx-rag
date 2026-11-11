use super::*;

mod lines;

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
