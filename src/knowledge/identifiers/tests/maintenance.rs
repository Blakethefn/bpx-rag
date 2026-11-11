//! Identifier-index maintenance under legacy writers.
//!
//! The `legacy` helpers repeat, statement for statement, the chunk writes that `master`
//! performs. A build that predates the identifier index runs exactly these statements, and
//! the database triggers must keep the identifier table consistent for them too.

use super::super::*;
use rusqlite::Connection;

pub(crate) mod legacy;

mod pending;
mod queue;
mod rebuild;

fn identifier_row(conn: &Connection, rowid: i64) -> Option<String> {
    conn.query_row(
        "SELECT words FROM knowledge_identifiers WHERE rowid = ?1",
        params![rowid],
        |row| row.get(0),
    )
    .ok()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// Simulate a version-1 database (the branch's earlier identifier index) with residue.
fn set_version_one(conn: &Connection) {
    conn.execute(
        "UPDATE knowledge_index_meta SET value = '1' WHERE key = 'identifier_index_version'",
        [],
    )
    .unwrap();
}
