use super::super::*;
use serde_json::json;

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
