use super::super::*;

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
