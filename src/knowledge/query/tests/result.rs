use super::super::*;
use super::{make_store, seed_chunk};

// ── knowledge_search (no filter) ─────────────────────────────────────────

#[test]
fn search_empty_db_returns_empty() {
    let store = make_store();
    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "anything", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty());
}

#[test]
fn search_finds_bm25_match() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "notes/test.md",
        "Fatigue Model",
        "The fatigue model applies minute-weighted decay to player performance",
        "{}",
    );

    let weights = SearchWeights::default();
    let results =
        knowledge_search(&store, "fatigue model", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].source_type, "knowledge");
    assert_eq!(results[0].source_label, "vault");
    assert_eq!(results[0].file_path, "notes/test.md");
}

#[test]
fn search_returns_correct_lines_format() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "test.md",
        "Title",
        "Content here",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "content", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].lines, "1-20");
}

#[test]
fn search_metadata_in_result() {
    let store = make_store();
    let meta = r#"{"frontmatter":{"type":"task","status":"active"}}"#;
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "meta.md",
        "Meta Test",
        "Content with metadata for testing purposes",
        meta,
    );

    let weights = SearchWeights::default();
    let results =
        knowledge_search(&store, "metadata testing", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    let fm = &results[0].metadata["frontmatter"];
    assert_eq!(fm["type"], "task");
    assert_eq!(fm["status"], "active");
}

// ── combined_search ──────────────────────────────────────────────────────

#[test]
fn search_snippet_truncated() {
    let store = make_store();
    let long_content = "word ".repeat(500); // ~2500 chars
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "long.md",
        "Long Content",
        &long_content,
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "word", None, None, 10, &weights, 100).unwrap();
    assert!(!results.is_empty());
    assert!(results[0].snippet.len() <= 104); // 100 + "..."
}

// ── dot_product ──────────────────────────────────────────────────────────

#[test]
fn test_dot_product_identical() {
    let a = vec![1.0, 0.0, 0.0];
    assert!((dot_product(&a, &a) - 1.0).abs() < 1e-6);
}

#[test]
fn test_dot_product_orthogonal() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![0.0, 1.0, 0.0];
    assert!(dot_product(&a, &b).abs() < 1e-6);
}
