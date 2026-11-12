use super::super::scrub::scrub_sensitive_indexed_files;
use super::super::*;
use crate::support::{add_source, make_store};
use tempfile::tempdir;

fn indexed_rel_paths(store: &KnowledgeStore, source_id: i64) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files WHERE source_id = ?1 ORDER BY rel_path")
        .unwrap();
    stmt.query_map(params![source_id], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

#[test]
fn sync_never_indexes_credential_files_or_key_material() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("notes")).unwrap();
    fs::create_dir_all(dir.path().join("secrets")).unwrap();
    fs::create_dir_all(dir.path().join(".ssh")).unwrap();
    fs::write(
        dir.path().join("notes/readme.md"),
        "# Readme\nkestrel public notes",
    )
    .unwrap();
    fs::write(dir.path().join(".env"), "DATABASE_URL=kestrel-private").unwrap();
    fs::write(dir.path().join("secrets/token.txt"), "kestrel token value").unwrap();
    fs::write(dir.path().join(".ssh/config"), "Host kestrel-bastion").unwrap();
    fs::write(dir.path().join("server.pem"), "-----BEGIN PRIVATE KEY-----").unwrap();
    // Built at runtime so the repository never contains a key-shaped literal.
    let fake_key = format!("AIza{}", "0123456789abcdefghijklmnopqrstuvwxy");
    fs::write(
        dir.path().join("leak.md"),
        format!("# Leak\nkestrel key {fake_key}"),
    )
    .unwrap();
    let source = add_source(&store, "test", dir.path());

    sync_source(&store, &source, None).unwrap();

    assert_eq!(
        indexed_rel_paths(&store, source.id),
        vec!["notes/readme.md".to_string()]
    );
    let leaked: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunks WHERE content LIKE '%kestrel-private%' OR content LIKE '%kestrel token%' OR content LIKE '%kestrel-bastion%' OR content LIKE '%AIza%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0, "no chunk holds credential text");
}

#[test]
fn scrub_removes_rows_that_match_the_policy_after_they_were_indexed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("readme.md"), "# Readme\nkestrel public").unwrap();
    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();

    // Simulate a row written before the policy existed.
    let conn = store.conn();
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, '.env', 'legacy', 32)",
        params![source.id],
    )
    .unwrap();
    let file_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('.env', 'DATABASE_URL=kestrel-legacy')",
        [],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
         VALUES (?1, ?2, ?3, 1, 1, '{}')",
        params![rowid, file_id, source.id],
    )
    .unwrap();

    scrub_sensitive_indexed_files(&store, Some(source.id)).unwrap();

    assert_eq!(
        indexed_rel_paths(&store, source.id),
        vec!["readme.md".to_string()]
    );
    let chunks_left: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunks WHERE content LIKE '%kestrel-legacy%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        chunks_left, 0,
        "scrub removes the chunks as well as the file row"
    );
}
