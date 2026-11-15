use super::super::super::*;
use super::super::{make_store, seed_chunk};

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
