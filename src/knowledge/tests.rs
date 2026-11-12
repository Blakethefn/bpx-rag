use crate::support::make_store;

mod remove;

#[test]
fn open_is_idempotent() {
    let _a = make_store();
    let _b = make_store();
}

#[test]
fn schema_tables_exist() {
    let store = make_store();
    let conn = store.conn();
    for table in &[
        "knowledge_sources",
        "knowledge_files",
        "knowledge_chunks",
        "knowledge_chunks_trigram",
        "knowledge_chunk_meta",
        "knowledge_embeddings",
        "knowledge_links",
    ] {
        let count: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','shadow') AND name = '{table}'"
                ),
                [],
                |r| r.get(0),
            )
            .expect(table);
        assert!(count > 0, "table '{table}' should exist in schema");
    }
}

#[test]
fn add_source_returns_id() {
    let store = make_store();
    let id = store
        .add_source("test", "/tmp/test", &[], &[])
        .expect("add_source");
    assert!(id > 0);
}

#[test]
fn add_source_duplicate_label_errors() {
    let store = make_store();
    store
        .add_source("test", "/tmp/test", &[], &[])
        .expect("first add");
    let result = store.add_source("test", "/tmp/other", &[], &[]);
    assert!(result.is_err(), "duplicate label should error");
}

#[test]
fn add_source_stores_enrichments_as_json() {
    let store = make_store();
    let enrichments = vec!["frontmatter".to_string(), "wikilinks".to_string()];
    store
        .add_source("e", "/tmp/e", &[], &enrichments)
        .expect("add");
    let src = store.get_source("e").unwrap().unwrap();
    assert_eq!(src.enrichments, enrichments);
}

#[test]
fn list_sources_empty() {
    let store = make_store();
    assert!(store.list_sources().unwrap().is_empty());
}

#[test]
fn list_sources_returns_added() {
    let store = make_store();
    store.add_source("alpha", "/tmp/alpha", &[], &[]).unwrap();
    store
        .add_source("beta", "/tmp/beta", &["**/*.md".to_string()], &[])
        .unwrap();
    let sources = store.list_sources().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].label, "alpha");
    assert_eq!(sources[1].label, "beta");
    assert_eq!(sources[1].patterns, vec!["**/*.md".to_string()]);
}

#[test]
fn get_source_returns_correct_fields() {
    let store = make_store();
    let enr = vec!["frontmatter".to_string()];
    let id = store
        .add_source("vault", "/tmp/vault", &["**/*.md".to_string()], &enr)
        .unwrap();
    let src = store.get_source("vault").unwrap().unwrap();
    assert_eq!(src.id, id);
    assert_eq!(src.label, "vault");
    assert_eq!(src.path, "/tmp/vault");
    assert_eq!(src.patterns, vec!["**/*.md".to_string()]);
    assert_eq!(src.enrichments, enr);
    assert_eq!(src.file_count, 0);
    assert_eq!(src.chunk_count, 0);
    assert!(src.last_sync.is_none());
}

#[test]
fn get_source_nonexistent_returns_none() {
    let store = make_store();
    assert!(store.get_source("nonexistent").unwrap().is_none());
}

#[test]
fn malformed_enrichments_are_reported_instead_of_read_as_none() {
    let store = make_store();
    store.add_source("vault", "/tmp/vault", &[], &[]).unwrap();
    store
        .conn()
        .execute(
            "UPDATE knowledge_sources SET enrichments = 'not json' WHERE label = 'vault'",
            [],
        )
        .unwrap();

    assert!(store.get_source("vault").is_err());
    assert!(store.list_sources().is_err());
}

#[test]
fn remove_source_deletes_record() {
    let store = make_store();
    store.add_source("gone", "/tmp/gone", &[], &[]).unwrap();
    store.remove_source("gone").expect("remove");
    assert!(store.get_source("gone").unwrap().is_none());
}

#[test]
fn remove_source_returns_zero_counts_when_no_files() {
    let store = make_store();
    store.add_source("empty", "/tmp/empty", &[], &[]).unwrap();
    let result = store.remove_source("empty").unwrap();
    assert_eq!(result.files_removed, 0);
    assert_eq!(result.chunks_removed, 0);
}

#[test]
fn remove_source_nonexistent_errors() {
    let store = make_store();
    assert!(store.remove_source("nosuchsource").is_err());
}

#[test]
fn get_related_chunks_unknown_file_is_empty() {
    let store = make_store();
    let result = store.get_related_chunks(999, 10, None).unwrap();
    assert!(result.is_empty());
}
