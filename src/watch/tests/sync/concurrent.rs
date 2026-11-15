use super::super::*;

#[test]
fn concurrent_manual_and_watched_sync_keep_the_database_consistent() {
    let fixture = fixture();
    let (handle, stop) = start(&fixture.db);
    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));

    for round in 0..10 {
        fs::write(
            fixture.source_dir.join("a.rs"),
            format!("pub fn round_{round:02}() -> u8 {{ {round} }}\n"),
        )
        .unwrap();
        let _writer = FileLock::acquire(&writer_lock_path(&fixture.db)).unwrap();
        let store = KnowledgeStore::open_at(&fixture.db).unwrap();
        let source = store.get_source("test").unwrap().unwrap();
        sync_source(&store, &source, None).unwrap();
    }

    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["round_09"]));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);

    let store = KnowledgeStore::open_at(&fixture.db).unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    let settled = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        (
            settled.files_added,
            settled.files_updated,
            settled.files_removed
        ),
        (0, 0, 0),
        "after the watcher stops, the index must already match the disk"
    );
    let integrity: String = store
        .conn()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}
