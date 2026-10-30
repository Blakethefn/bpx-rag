use super::*;



#[test]
fn test_preview() {
    let content = "a".repeat(500);
    let result = preview(&content, 100);
    assert!(result.contains("truncated"));
    assert!(result.contains("500 bytes total"));
}
