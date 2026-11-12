use super::super::super::*;
use super::legacy;
use super::{count, identifier_row};
use crate::knowledge::schema::apply_schema;

#[test]
fn rolled_back_legacy_write_leaves_no_pending_entry() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    legacy::insert_chunk(&conn, 1, 1, "a.ts", "function getUserById() {}");
    conn.execute_batch("ROLLBACK").unwrap();

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM knowledge_chunk_meta"), 0);
}

#[test]
fn pending_entry_without_metadata_is_dropped_not_indexed() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO knowledge_identifier_pending (chunk_rowid) VALUES (42)",
        [],
    )
    .unwrap();

    ensure_identifier_index(&conn).unwrap();

    assert_eq!(identifier_row(&conn, 42), None);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
}

#[test]
fn deleted_chunk_leaves_no_identifier_row_for_its_rowid_before_any_reopen() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    assert_eq!(identifier_row(&conn, old), None);
    let reused = legacy::insert_chunk(&conn, 2, 1, "notes.md", "pizza dough");
    assert_eq!(reused, old);
    assert_eq!(
        identifier_row(&conn, reused),
        None,
        "the reused rowid must not carry the deleted chunk's words before the queue runs"
    );
}
