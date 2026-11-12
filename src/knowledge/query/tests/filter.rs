use super::super::*;
use super::{make_store, seed_chunk};

#[test]
fn search_with_frontmatter_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "task1.md",
        "Active Task",
        "This is an active task about implementation",
        r#"{"frontmatter":{"type":"task","status":"active"}}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "note1.md",
        "Random Note",
        "This is a random note about implementation",
        r#"{"frontmatter":{"type":"note","status":"draft"}}"#,
    );

    let weights = SearchWeights::default();

    let results = knowledge_search(
        &store,
        "implementation",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Active Task");
}

#[test]
fn search_with_source_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "v.md",
        "Vault Note",
        "Content from vault about algorithms",
        "{}",
    );
    seed_chunk(
        &store,
        "docs",
        "/tmp/docs",
        "d.md",
        "Docs Note",
        "Content from docs about algorithms",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "algorithms",
        Some("source:vault"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "vault");
}

#[test]
fn search_with_tag_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "tagged.md",
        "Tagged Note",
        "Content about prediction models and forecasting",
        r#"{"frontmatter":{"tags":["implementation","system3"]}}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "untagged.md",
        "Untagged Note",
        "Content about prediction models and estimation",
        r#"{"frontmatter":{"tags":["review"]}}"#,
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "prediction",
        Some("tag:implementation"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Tagged Note");
}

#[test]
fn search_with_folder_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "projects/foo.md",
        "Project Note",
        "Content about project setup and configuration",
        r#"{"folder_tags":["01-projects","foo"]}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "logs/bar.md",
        "Log Note",
        "Content about debugging and configuration",
        r#"{"folder_tags":["06-logs","debug"]}"#,
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "configuration",
        Some("folder:01-projects"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Project Note");
}

#[test]
fn search_with_source_label_param() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "v.md",
        "Vault Item",
        "Searchable content about architecture",
        "{}",
    );
    seed_chunk(
        &store,
        "docs",
        "/tmp/docs",
        "d.md",
        "Docs Item",
        "Searchable content about architecture",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "architecture",
        None,
        Some("docs"),
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "docs");
}
