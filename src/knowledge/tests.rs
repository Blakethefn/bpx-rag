use super::*;

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

// ── Schema / open ────────────────────────────────────────────────────────

#[test]
fn open_is_idempotent() {
    // Opening twice should not error.
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

// ── add_source ───────────────────────────────────────────────────────────

#[test]
fn add_source_returns_id() {
    let store = make_store();
    let id = store
        .add_source("test", "/tmp/test", None, &[])
        .expect("add_source");
    assert!(id > 0);
}

#[test]
fn add_source_duplicate_label_errors() {
    let store = make_store();
    store
        .add_source("test", "/tmp/test", None, &[])
        .expect("first add");
    let result = store.add_source("test", "/tmp/other", None, &[]);
    assert!(result.is_err(), "duplicate label should error");
}

#[test]
fn add_source_stores_enrichments_as_json() {
    let store = make_store();
    let enrichments = vec!["frontmatter".to_string(), "wikilinks".to_string()];
    store
        .add_source("e", "/tmp/e", None, &enrichments)
        .expect("add");
    let src = store.get_source("e").unwrap().unwrap();
    assert_eq!(src.enrichments, enrichments);
}

// ── list_sources / get_source ─────────────────────────────────────────────

#[test]
fn list_sources_empty() {
    let store = make_store();
    assert!(store.list_sources().unwrap().is_empty());
}

#[test]
fn list_sources_returns_added() {
    let store = make_store();
    store.add_source("alpha", "/tmp/alpha", None, &[]).unwrap();
    store
        .add_source("beta", "/tmp/beta", Some("**/*.md"), &[])
        .unwrap();
    let sources = store.list_sources().unwrap();
    assert_eq!(sources.len(), 2);
    // Ordered by label
    assert_eq!(sources[0].label, "alpha");
    assert_eq!(sources[1].label, "beta");
    assert_eq!(sources[1].glob, Some("**/*.md".to_string()));
}

#[test]
fn get_source_returns_correct_fields() {
    let store = make_store();
    let enr = vec!["frontmatter".to_string()];
    let id = store
        .add_source("vault", "/tmp/vault", Some("**/*.md"), &enr)
        .unwrap();
    let src = store.get_source("vault").unwrap().unwrap();
    assert_eq!(src.id, id);
    assert_eq!(src.label, "vault");
    assert_eq!(src.path, "/tmp/vault");
    assert_eq!(src.glob, Some("**/*.md".to_string()));
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

// ── remove_source ─────────────────────────────────────────────────────────

#[test]
fn remove_source_deletes_record() {
    let store = make_store();
    store.add_source("gone", "/tmp/gone", None, &[]).unwrap();
    store.remove_source("gone").expect("remove");
    assert!(store.get_source("gone").unwrap().is_none());
}

#[test]
fn remove_source_returns_zero_counts_when_no_files() {
    let store = make_store();
    store.add_source("empty", "/tmp/empty", None, &[]).unwrap();
    let result = store.remove_source("empty").unwrap();
    assert_eq!(result.files_removed, 0);
    assert_eq!(result.chunks_removed, 0);
}

#[test]
fn remove_source_nonexistent_errors() {
    let store = make_store();
    assert!(store.remove_source("nosuchsource").is_err());
}

// ── source_for_path ───────────────────────────────────────────────────────

// ── embedder ──────────────────────────────────────────────────────────────

// ── get_related_chunks ───────────────────────────────────────────────────

#[test]
fn get_related_chunks_unknown_file_is_empty() {
    let store = make_store();
    let result = store.get_related_chunks(999, 10, None).unwrap();
    assert!(result.is_empty());
}
