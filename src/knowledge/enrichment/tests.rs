use super::*;
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

// ── build_enrichment_fn ──────────────────────────────────────────────────

#[test]
fn build_empty_list_returns_none() {
    let result = build_enrichment_fn(&[], Path::new("/base"));
    assert!(result.is_none());
}

#[test]
fn build_all_three_enrichments() {
    let enrichments = vec![
        "frontmatter".to_string(),
        "wikilinks".to_string(),
        "folder_tags".to_string(),
    ];
    let func = build_enrichment_fn(&enrichments, Path::new("/vault")).expect("should return Some");

    let content = "---\ntype: task\n---\nSee [[other]]";
    let path = Path::new("/vault/01-projects/note.md");
    let result = func(content, path);

    let obj = result.as_object().expect("should be object");
    assert!(obj.contains_key("frontmatter"));
    assert!(obj.contains_key("wikilinks"));
    assert!(obj.contains_key("folder_tags"));

    assert_eq!(obj["frontmatter"], json!({"type": "task"}));
    assert_eq!(obj["wikilinks"], json!(["other"]));
    assert_eq!(obj["folder_tags"], json!(["01-projects"]));
}

#[test]
fn build_frontmatter_only() {
    let enrichments = vec!["frontmatter".to_string()];
    let func = build_enrichment_fn(&enrichments, Path::new("/base")).expect("should return Some");

    let content = "---\nstatus: done\n---\nBody";
    let result = func(content, Path::new("/base/file.md"));

    let obj = result.as_object().expect("should be object");
    assert!(obj.contains_key("frontmatter"));
    assert!(!obj.contains_key("wikilinks"));
    assert!(!obj.contains_key("folder_tags"));
}

#[test]
fn build_wikilinks_only() {
    let enrichments = vec!["wikilinks".to_string()];
    let func = build_enrichment_fn(&enrichments, Path::new("/base")).expect("should return Some");

    let content = "Link to [[target]]";
    let result = func(content, Path::new("/base/file.md"));

    let obj = result.as_object().expect("should be object");
    assert!(!obj.contains_key("frontmatter"));
    assert!(obj.contains_key("wikilinks"));
    assert!(!obj.contains_key("folder_tags"));
}

#[test]
fn build_folder_tags_only() {
    let enrichments = vec!["folder_tags".to_string()];
    let func = build_enrichment_fn(&enrichments, Path::new("/vault")).expect("should return Some");

    let content = "No links, no frontmatter";
    let result = func(content, Path::new("/vault/deep/path/file.md"));

    let obj = result.as_object().expect("should be object");
    assert!(!obj.contains_key("frontmatter"));
    assert!(!obj.contains_key("wikilinks"));
    assert!(obj.contains_key("folder_tags"));
    assert_eq!(obj["folder_tags"], json!(["deep", "path"]));
}

#[test]
fn build_unknown_enrichment_ignored() {
    // Unknown enrichments are ignored; valid ones still work
    let enrichments = vec!["frontmatter".to_string(), "nonexistent".to_string()];
    let func = build_enrichment_fn(&enrichments, Path::new("/base")).expect("should return Some");

    let content = "---\ntype: test\n---\nBody";
    let result = func(content, Path::new("/base/file.md"));

    let obj = result.as_object().expect("should be object");
    assert!(obj.contains_key("frontmatter"));
    assert_eq!(obj["frontmatter"], json!({"type": "test"}));
}

#[test]
fn build_only_unknown_returns_empty_object() {
    let enrichments = vec!["bogus".to_string()];
    let func = build_enrichment_fn(&enrichments, Path::new("/base"))
        .expect("should return Some for non-empty list");

    let content = "Hello world";
    let result = func(content, Path::new("/base/file.md"));
    assert_eq!(result, json!({}));
}

#[test]
fn build_omits_empty_enrichment_keys() {
    // When content has no frontmatter and no wikilinks, those keys
    // should be absent from the output (not present with empty values).
    let enrichments = vec![
        "frontmatter".to_string(),
        "wikilinks".to_string(),
        "folder_tags".to_string(),
    ];
    let func = build_enrichment_fn(&enrichments, Path::new("/vault")).expect("should return Some");

    let content = "Plain text, no frontmatter, no links";
    let path = Path::new("/vault/root.md");
    let result = func(content, path);

    let obj = result.as_object().expect("should be object");
    // No frontmatter → key absent
    assert!(!obj.contains_key("frontmatter"));
    // No wikilinks → key absent
    assert!(!obj.contains_key("wikilinks"));
    // Root file → no folder tags → key absent
    assert!(!obj.contains_key("folder_tags"));
}

#[test]
fn validate_enrichments_accepts_known_names() {
    let names = vec!["frontmatter".to_string(), "wikilinks".to_string()];
    assert!(validate_enrichments(&names).is_ok());
}

#[test]
fn validate_enrichments_rejects_unknown_name_with_choices() {
    let names = vec!["frontmatter".to_string(), "wikilnks".to_string()];
    let error = validate_enrichments(&names).unwrap_err().to_string();
    assert!(error.contains("unknown enrichment 'wikilnks'"));
    assert!(error.contains("frontmatter, wikilinks, folder_tags"));
}
