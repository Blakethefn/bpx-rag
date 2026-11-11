use super::super::*;

#[test]
fn applying_schema_backfills_identifier_words_for_chunks_that_predate_the_index() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('users.ts', 'export function getUserById(id) {}')",
        [],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    super::simulate_pre_identifier_index(&conn);

    apply_schema(&conn).unwrap();

    let words: String = conn
        .query_row(
            "SELECT words FROM knowledge_identifiers WHERE rowid = ?1",
            [rowid],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(words, "get user by id");
}

#[test]
fn identifier_backfill_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('a.ts', 'function getUserById() {}')",
        [],
    )
    .unwrap();
    super::simulate_pre_identifier_index(&conn);

    apply_schema(&conn).unwrap();
    apply_schema(&conn).unwrap();

    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM knowledge_identifiers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 1);
}
