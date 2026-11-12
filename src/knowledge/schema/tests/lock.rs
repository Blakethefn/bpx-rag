use crate::knowledge::{query, sync, KnowledgeStore};
use rusqlite::Connection;
use tempfile::tempdir;

fn up_to_date_database(dir: &std::path::Path) -> std::path::PathBuf {
    let db_path = dir.join("knowledge.db");
    let source_dir = dir.join("tree");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::write(
        source_dir.join("note.md"),
        "# Note\n\nThe holographic rendering pipeline.\n",
    )
    .unwrap();
    let store = KnowledgeStore::open_at(&db_path).unwrap();
    store
        .add_source("t", &source_dir.to_string_lossy(), &[], &[])
        .unwrap();
    let source = store.get_source("t").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();
    db_path
}

fn hold_write_lock(db_path: &std::path::Path) -> Connection {
    let holder = Connection::open(db_path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    holder
}

#[test]
fn schema_on_an_up_to_date_database_does_not_take_the_write_lock() {
    let dir = tempdir().unwrap();
    let db_path = up_to_date_database(dir.path());
    let _holder = hold_write_lock(&db_path);

    let conn = Connection::open(&db_path).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    super::apply_schema(&conn).expect("applying the schema must not need the write lock");
}

#[test]
fn search_succeeds_while_another_connection_holds_the_write_lock() {
    let dir = tempdir().unwrap();
    let db_path = up_to_date_database(dir.path());
    let _holder = hold_write_lock(&db_path);

    let store = KnowledgeStore::open_at(&db_path)
        .expect("opening an up-to-date database must not wait for the write lock");
    let results = query::knowledge_search(
        &store,
        "holographic rendering",
        None,
        None,
        10,
        &query::SearchWeights::default(),
        2000,
    )
    .unwrap();
    assert!(!results.is_empty(), "the indexed file must be found");
}
