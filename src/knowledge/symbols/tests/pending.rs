use super::super::*;
use super::{count, exact, lookup_with, sync};
use crate::knowledge::query::{knowledge_search_report, SearchWeights};
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn default_search_does_not_read_the_symbol_index() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "/// Loads the configured user record.\npub fn load_user_record() -> u8 { 1 }\n",
    )
    .unwrap();
    add_source(&store, "test", dir.path());
    sync(&store, "test");

    let weights = SearchWeights::default();
    let search = |store: &KnowledgeStore| {
        knowledge_search_report(store, "user record", None, None, 10, &weights, 200)
            .unwrap()
            .results
            .iter()
            .map(|result| (result.file_path.clone(), result.lines.clone(), result.score))
            .collect::<Vec<_>>()
    };
    let before = search(&store);
    store
        .conn()
        .execute_batch("DELETE FROM knowledge_symbols")
        .unwrap();
    let after = search(&store);
    assert!(!before.is_empty());
    assert_eq!(before, after);
}

#[test]
fn database_with_current_symbol_rows_reports_them_pending_after_upgrade_until_sync() {
    // A database written by the build before `symbols_hash` existed has symbol rows at
    // `symbol_version = 1` and no hash column. Opening it with this version adds the column
    // as NULL, because the migration cannot prove the rows still match the content. Lookup
    // therefore reports the source as pending, and one sync restores it without re-chunking.
    let dir = tempdir().unwrap();
    let source_dir = dir.path().join("tree");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("lib.rs"), "pub fn upgraded_entry() {}\n").unwrap();
    let db_path = dir.path().join("knowledge.db");
    {
        let store = KnowledgeStore::open_at(&db_path).unwrap();
        add_source(&store, "test", &source_dir);
        sync(&store, "test");
        assert_eq!(exact(&store, "upgraded_entry").len(), 1);
    }
    {
        let raw = rusqlite::Connection::open(&db_path).unwrap();
        raw.execute_batch("ALTER TABLE knowledge_files DROP COLUMN symbols_hash")
            .unwrap();
        let version: i64 = raw
            .query_row("SELECT symbol_version FROM knowledge_files", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SYMBOL_INDEX_VERSION, "fixture must look like v1");
    }

    let store = KnowledgeStore::open_at(&db_path).unwrap();

    let symbol_rows = count(
        &store,
        "SELECT COUNT(*) FROM knowledge_symbols ks
         JOIN knowledge_sources s ON s.id = ks.source_id WHERE s.label = ?1",
        "test",
    );
    assert_eq!(symbol_rows, 1, "migration must keep the stored symbol rows");
    let pending = lookup_with(&store, "upgraded_entry", MatchMode::Exact, None, 20);
    assert!(pending.hits.is_empty(), "stale-unknown rows must be hidden");
    assert_eq!(
        pending.pending,
        vec![PendingSymbols {
            source_label: "test".to_string(),
            files: 1
        }]
    );

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();
    assert_eq!(result.files_updated, 0, "restoring must not re-chunk");
    assert_eq!(result.chunks_indexed, 0);

    let restored = lookup_with(&store, "upgraded_entry", MatchMode::Exact, None, 20);
    assert_eq!(restored.hits.len(), 1);
    assert!(restored.pending.is_empty());
}
