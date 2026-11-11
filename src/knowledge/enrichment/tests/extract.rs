use super::super::*;
use serde_json::json;

// ── extract_frontmatter ──────────────────────────────────────────────────

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

// ── extract_wikilinks ────────────────────────────────────────────────────

#[test]
fn wikilinks_simple() {
    let content = "See [[projects/foo]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["projects/foo"]));
}

#[test]
fn wikilinks_with_alias() {
    let content = "Link [[target|alias]] here";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["target"]));
}

#[test]
fn wikilinks_multiple() {
    let content = "Two [[a]] and [[b]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["a", "b"]));
}

#[test]
fn wikilinks_nested_brackets() {
    // First ]] closes the link — inner [[ is just content
    let content = "Nested [[a|[[b]]]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["a"]));
}

#[test]
fn wikilinks_none() {
    let content = "No links here";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!([]));
}

#[test]
fn wikilinks_deduplicated() {
    let content = "Duplicate [[a]] and [[a]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["a"]));
}

#[test]
fn wikilinks_unclosed() {
    let content = "Unclosed [[link";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!([]));
}

#[test]
fn wikilinks_empty_target() {
    let content = "Empty [[]] link";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!([]));
}

#[test]
fn wikilinks_adjacent() {
    let content = "[[a]][[b]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["a", "b"]));
}

#[test]
fn wikilinks_in_frontmatter() {
    // Wikilinks inside frontmatter are still extracted (by design)
    let content = "---\nproject: \"[[myproject]]\"\n---\nSee [[other]]";
    let result = extract_wikilinks(content);
    assert_eq!(result, json!(["myproject", "other"]));
}

// ── extract_folder_tags ──────────────────────────────────────────────────

#[test]
fn folder_tags_multi_level() {
    let file = Path::new("/vault/01-projects/bpcontext/specs/foo.md");
    let base = Path::new("/vault");
    let result = extract_folder_tags(file, base);
    assert_eq!(result, json!(["01-projects", "bpcontext", "specs"]));
}

#[test]
fn folder_tags_single_level() {
    let file = Path::new("/vault/tasks/task.md");
    let base = Path::new("/vault");
    let result = extract_folder_tags(file, base);
    assert_eq!(result, json!(["tasks"]));
}

#[test]
fn folder_tags_root_file() {
    let file = Path::new("/vault/root-note.md");
    let base = Path::new("/vault");
    let result = extract_folder_tags(file, base);
    assert_eq!(result, json!([]));
}

#[test]
fn folder_tags_path_not_under_base() {
    let file = Path::new("/other/path/file.md");
    let base = Path::new("/vault");
    let result = extract_folder_tags(file, base);
    assert_eq!(result, json!([]));
}
