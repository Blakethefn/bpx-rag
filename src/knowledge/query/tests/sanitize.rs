use super::super::lexical::{sanitize_fts_query, sanitize_trigram_query};
use super::super::rank::truncate_snippet;

#[test]
fn test_sanitize_fts_query() {
    assert_eq!(
        sanitize_fts_query("hello world"),
        "\"hello world\" OR \"hello\" OR \"world\""
    );
    assert_eq!(
        sanitize_fts_query("error: can't find"),
        "\"error can t find\" OR \"error\" OR \"can\" OR \"t\" OR \"find\""
    );
    assert_eq!(
        sanitize_fts_query("fn main()"),
        "\"fn main\" OR \"fn\" OR \"main\""
    );
}

#[test]
fn test_sanitize_trigram_query() {
    assert_eq!(sanitize_trigram_query("hello"), "\"hello\"");
    assert_eq!(sanitize_trigram_query(""), "");
}

#[test]
fn test_truncate_snippet_short() {
    let s = "short text";
    assert_eq!(truncate_snippet(s, 100), "short text");
}

#[test]
fn test_truncate_snippet_at_word_boundary() {
    let s = "the quick brown fox jumps over the lazy dog";
    let result = truncate_snippet(s, 20);
    assert!(result.ends_with("..."));
    assert!(result.len() <= 24);
}

#[test]
fn test_truncate_snippet_preserves_utf8_boundaries() {
    let result = truncate_snippet("before 🧑‍💻 after", 9);
    assert_eq!(result, "before...");
}
