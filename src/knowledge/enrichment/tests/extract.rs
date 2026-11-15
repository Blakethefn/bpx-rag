use super::super::*;
use serde_json::json;

mod links;

#[test]
fn frontmatter_standard_fields() {
    let content = "---\ntype: task\nstatus: active\n---\nBody text";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({"type": "task", "status": "active"}));
}

#[test]
fn frontmatter_with_tags_array() {
    let content = "---\ntags:\n  - foo\n  - bar\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({"tags": ["foo", "bar"]}));
}

#[test]
fn frontmatter_with_wikilink_value() {
    let content = "---\nproject: \"[[link]]\"\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({"project": "[[link]]"}));
}

#[test]
fn frontmatter_no_frontmatter() {
    let content = "No frontmatter here";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({}));
}

#[test]
fn frontmatter_malformed_yaml() {
    let content = "---\ninvalid: yaml: [broken\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({}));
}

#[test]
fn frontmatter_empty_between_fences() {
    let content = "---\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({}));
}

#[test]
fn frontmatter_non_object_yaml() {
    let content = "---\njust a string\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({}));
}

#[test]
fn frontmatter_no_closing_fence() {
    let content = "---\ntype: task\nstatus: active\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({}));
}

#[test]
fn frontmatter_leading_whitespace() {
    let content = "  ---\ntype: task\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({"type": "task"}));
}

#[test]
fn frontmatter_multiple_types() {
    let content = "---\ncount: 42\nactive: true\nname: test\n---\n";
    let result = extract_frontmatter(content);
    assert_eq!(result, json!({"count": 42, "active": true, "name": "test"}));
}
