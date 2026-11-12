use super::super::super::*;
use super::legacy;
use super::{count, identifier_row};
use crate::knowledge::schema::apply_schema;

#[test]
fn legacy_insert_is_queued_and_indexed_on_next_open() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let rowid = legacy::insert_chunk(&conn, 1, 1, "users.ts", "export function getUserById() {}");

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        1
    );
    assert_eq!(identifier_row(&conn, rowid), None);

    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, rowid).as_deref(),
        Some("get user by id")
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifier_pending"),
        0
    );
}

#[test]
fn legacy_delete_removes_the_identifier_row_of_its_chunks() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let rowid = legacy::insert_chunk(&conn, 7, 1, "users.ts", "function getUserById() {}");
    ensure_identifier_index(&conn).unwrap();
    assert!(identifier_row(&conn, rowid).is_some());

    legacy::delete_file(&conn, 7);

    assert_eq!(
        identifier_row(&conn, rowid),
        None,
        "orphaned identifier row"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        0
    );
}

#[test]
fn legacy_source_removal_clears_identifier_rows() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    legacy::insert_chunk(&conn, 1, 9, "a.ts", "function getUserById() {}");
    legacy::insert_chunk(&conn, 2, 9, "b.ts", "function parseHTTPResponse() {}");
    ensure_identifier_index(&conn).unwrap();

    legacy::remove_source(&conn, 9);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        0
    );
}

#[test]
fn reused_rowid_does_not_inherit_the_words_of_a_deleted_chunk() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    let reused = legacy::insert_chunk(&conn, 2, 1, "notes.md", "pizza dough and a hot oven");
    assert_eq!(reused, old, "test requires a reused rowid");
    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, reused).as_deref(),
        Some(""),
        "a plain-prose chunk must not carry the deleted chunk's identifier words"
    );
}

#[test]
fn reused_rowid_with_a_compound_identifier_gets_its_own_words() {
    let conn = Connection::open_in_memory().unwrap();
    apply_schema(&conn).unwrap();
    let old = legacy::insert_chunk(&conn, 1, 1, "billing.ts", "function buildInvoiceTotal() {}");
    ensure_identifier_index(&conn).unwrap();
    legacy::delete_file(&conn, 1);

    let reused = legacy::insert_chunk(&conn, 2, 1, "users.ts", "function getUserById() {}");
    assert_eq!(reused, old);
    ensure_identifier_index(&conn).unwrap();

    assert_eq!(
        identifier_row(&conn, reused).as_deref(),
        Some("get user by id")
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM knowledge_identifiers"),
        1
    );
}
