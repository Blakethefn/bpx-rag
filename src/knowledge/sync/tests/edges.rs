use super::super::*;
use super::{make_store, register_source};
use tempfile::tempdir;

fn code_edge_count(store: &KnowledgeStore, source_id: i64) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_links kl
             JOIN knowledge_files kf ON kf.id = kl.source_file_id
             WHERE kf.source_id = ?1 AND kl.link_type LIKE 'code:%'",
            params![source_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn sync_stores_rust_module_relations_and_drops_them_when_removed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "mod util;\npub fn run() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/util.rs"),
        "pub fn helper() -> u8 { 1 }\n",
    )
    .unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    let lib_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = 'src/lib.rs'",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    let related = store.related_files(lib_id, 10).unwrap();
    assert_eq!(related, vec!["src/util.rs".to_string()]);
    assert!(code_edge_count(&store, source.id) >= 1);

    fs::write(dir.path().join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    sync_source(&store, &source, None).unwrap();
    assert!(
        store.related_files(lib_id, 10).unwrap().is_empty(),
        "edge is dropped once lib.rs no longer declares the module"
    );
}

#[test]
fn unchanged_sync_keeps_code_edges_without_rereading_files() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "mod util;\n").unwrap();
    fs::write(dir.path().join("util.rs"), "pub fn helper() {}\n").unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    let edges = code_edge_count(&store, source.id);
    assert!(edges >= 1);

    let second = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        second.files_added + second.files_updated + second.files_removed,
        0
    );
    assert_eq!(code_edge_count(&store, source.id), edges);
}
