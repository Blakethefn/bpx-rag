use super::*;

// ── parse_filter ─────────────────────────────────────────────────────────

#[test]
fn parse_none_returns_empty() {
    assert!(parse_filter(None).unwrap().is_empty());
}

#[test]
fn parse_empty_string_returns_empty() {
    assert!(parse_filter(Some("")).unwrap().is_empty());
}

#[test]
fn parse_whitespace_only_returns_empty() {
    assert!(parse_filter(Some("   ")).unwrap().is_empty());
}

#[test]
fn parse_no_colon_tokens_fail_closed() {
    assert!(parse_filter(Some("invalid_no_colon")).is_err());
}

#[test]
fn parse_source_predicate() {
    let preds = parse_filter(Some("source:vault")).unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0], FilterPredicate::Source("vault".to_string()));
}

#[test]
fn parse_tag_predicate() {
    let preds = parse_filter(Some("tag:implementation")).unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0], FilterPredicate::Tag("implementation".to_string()));
}

#[test]
fn parse_folder_predicate() {
    let preds = parse_filter(Some("folder:01-projects")).unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(preds[0], FilterPredicate::Folder("01-projects".to_string()));
}

#[test]
fn parse_frontmatter_predicate() {
    let preds = parse_filter(Some("type:task")).unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(
        preds[0],
        FilterPredicate::Frontmatter {
            key: "type".to_string(),
            value: "task".to_string(),
        }
    );
}

#[test]
fn parse_multiple_predicates_anded() {
    let preds = parse_filter(Some("type:task status:active")).unwrap();
    assert_eq!(preds.len(), 2);
    assert_eq!(
        preds[0],
        FilterPredicate::Frontmatter {
            key: "type".to_string(),
            value: "task".to_string(),
        }
    );
    assert_eq!(
        preds[1],
        FilterPredicate::Frontmatter {
            key: "status".to_string(),
            value: "active".to_string(),
        }
    );
}

#[test]
fn parse_mixed_predicates() {
    let preds = parse_filter(Some("source:vault type:task tag:foo folder:01-projects")).unwrap();
    assert_eq!(preds.len(), 4);
    assert_eq!(preds[0], FilterPredicate::Source("vault".to_string()));
    assert_eq!(
        preds[1],
        FilterPredicate::Frontmatter {
            key: "type".to_string(),
            value: "task".to_string(),
        }
    );
    assert_eq!(preds[2], FilterPredicate::Tag("foo".to_string()));
    assert_eq!(preds[3], FilterPredicate::Folder("01-projects".to_string()));
}

#[test]
fn parse_rejects_empty_key_or_value() {
    assert!(parse_filter(Some(":value")).is_err());
    assert!(parse_filter(Some("key:")).is_err());
}

#[test]
fn parse_key_with_special_chars_rejected() {
    // Keys with special characters should be rejected (SQL injection prevention)
    assert!(parse_filter(Some("key';DROP:evil")).is_err());

    assert!(parse_filter(Some("key.sub:value")).is_err());

    assert!(parse_filter(Some("key space:value")).is_err());
}

#[test]
fn parse_key_with_hyphen_accepted() {
    let preds = parse_filter(Some("my-key:value")).unwrap();
    assert_eq!(preds.len(), 1);
    assert_eq!(
        preds[0],
        FilterPredicate::Frontmatter {
            key: "my-key".to_string(),
            value: "value".to_string(),
        }
    );
}

#[test]
fn parse_key_with_underscore_accepted() {
    let preds = parse_filter(Some("my_key:value")).unwrap();
    assert_eq!(preds.len(), 1);
}

// ── validate_key ─────────────────────────────────────────────────────────

#[test]
fn validate_key_alphanumeric() {
    assert!(validate_key("type"));
    assert!(validate_key("status123"));
}

#[test]
fn validate_key_with_underscore_hyphen() {
    assert!(validate_key("my_key"));
    assert!(validate_key("my-key"));
}

#[test]
fn validate_key_rejects_special_chars() {
    assert!(!validate_key("key'"));
    assert!(!validate_key("key;"));
    assert!(!validate_key("key.sub"));
    assert!(!validate_key("key space"));
    assert!(!validate_key(""));
}

// ── predicates_to_sql ────────────────────────────────────────────────────

#[test]
fn sql_empty_predicates() {
    let (clause, params) = predicates_to_sql(&[], 0);
    assert!(clause.is_empty());
    assert!(params.is_empty());
}

#[test]
fn sql_source_predicate() {
    let preds = vec![FilterPredicate::Source("vault".to_string())];
    let (clause, params) = predicates_to_sql(&preds, 0);
    assert!(clause.contains("knowledge_sources"));
    assert!(clause.contains("label = ?1"));
    assert_eq!(params, vec!["vault"]);
}

#[test]
fn sql_frontmatter_predicate() {
    let preds = vec![FilterPredicate::Frontmatter {
        key: "type".to_string(),
        value: "task".to_string(),
    }];
    let (clause, params) = predicates_to_sql(&preds, 0);
    assert!(clause.contains("json_extract(kcm.metadata, '$.frontmatter.type')"));
    assert!(clause.contains("= ?1"));
    assert_eq!(params, vec!["task"]);
}

#[test]
fn sql_tag_predicate() {
    let preds = vec![FilterPredicate::Tag("impl".to_string())];
    let (clause, params) = predicates_to_sql(&preds, 0);
    assert!(clause.contains("json_each"));
    assert!(clause.contains("'$.frontmatter.tags'"));
    assert!(clause.contains("value = ?1"));
    assert_eq!(params, vec!["impl"]);
}

#[test]
fn sql_folder_predicate() {
    let preds = vec![FilterPredicate::Folder("01-projects".to_string())];
    let (clause, params) = predicates_to_sql(&preds, 0);
    assert!(clause.contains("json_each"));
    assert!(clause.contains("'$.folder_tags'"));
    assert!(clause.contains("value = ?1"));
    assert_eq!(params, vec!["01-projects"]);
}

#[test]
fn sql_multiple_predicates_joined_with_and() {
    let preds = vec![
        FilterPredicate::Frontmatter {
            key: "type".to_string(),
            value: "task".to_string(),
        },
        FilterPredicate::Frontmatter {
            key: "status".to_string(),
            value: "active".to_string(),
        },
    ];
    let (clause, params) = predicates_to_sql(&preds, 0);
    assert!(clause.contains(" AND "));
    assert!(clause.contains("?1"));
    assert!(clause.contains("?2"));
    assert_eq!(params.len(), 2);
}

#[test]
fn sql_param_offset_respected() {
    let preds = vec![FilterPredicate::Source("vault".to_string())];
    let (clause, params) = predicates_to_sql(&preds, 3);
    // With offset 3, first param should be ?4
    assert!(clause.contains("?4"));
    assert_eq!(params.len(), 1);
}
