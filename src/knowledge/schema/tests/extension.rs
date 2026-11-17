use super::super::migrate::ensure_file_extension;
use rusqlite::{params, Connection};

#[test]
fn extension_migration_backfills_existing_and_later_legacy_rows() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE knowledge_files (id INTEGER PRIMARY KEY, rel_path TEXT NOT NULL);
         INSERT INTO knowledge_files (rel_path) VALUES ('docs/README.MD'), ('Makefile'), ('.env');",
    )
    .unwrap();

    ensure_file_extension(&conn).unwrap();
    let extensions: Vec<_> = conn
        .prepare("SELECT rel_path, extension FROM knowledge_files ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        extensions,
        [
            ("docs/README.MD".into(), "md".into()),
            ("Makefile".into(), "".into()),
            (".env".into(), "".into())
        ]
    );

    conn.execute(
        "INSERT INTO knowledge_files (rel_path) VALUES (?1)",
        params!["new/file.ts"],
    )
    .unwrap();
    ensure_file_extension(&conn).unwrap();
    let extension: String = conn
        .query_row(
            "SELECT extension FROM knowledge_files WHERE rel_path = ?1",
            params!["new/file.ts"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(extension, "ts");
}
