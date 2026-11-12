use super::super::{apply_schema, check_in, ANALYSIS_STATUS_CHECK};
use crate::knowledge::resolution::{ENTRY_KINDS, RESOLUTION_STATUSES};
use rusqlite::{params, Connection};
use std::path::Path;

fn table_sql(conn: &Connection, name: &str) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )
    .unwrap()
}

fn open() -> Connection {
    let conn = crate::db::open_db(Path::new(":memory:")).unwrap();
    apply_schema(&conn).unwrap();
    conn
}

#[test]
fn checks_are_built_from_the_shared_catalogs() {
    assert_eq!(
        ANALYSIS_STATUS_CHECK,
        "CHECK (status IN ('complete', 'partial', 'incomplete'))"
    );
    assert_eq!(
        check_in("kind", ENTRY_KINDS),
        "CHECK (kind IN ('program', 'test', 'library_root'))"
    );
    assert_eq!(
        check_in("status", RESOLUTION_STATUSES),
        "CHECK (status IN ('resolved', 'ambiguous', 'unresolved'))"
    );

    let conn = open();
    let analysis = table_sql(&conn, "knowledge_analysis");
    let entries = table_sql(&conn, "knowledge_entries");
    let resolutions = table_sql(&conn, "knowledge_resolutions");
    assert!(analysis.contains(ANALYSIS_STATUS_CHECK), "{analysis}");
    assert!(
        entries.contains(&check_in("kind", ENTRY_KINDS)),
        "{entries}"
    );
    assert!(
        resolutions.contains(&check_in("status", RESOLUTION_STATUSES)),
        "{resolutions}"
    );

    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    for (id, status) in [(1, "complete"), (2, "partial"), (3, "incomplete")] {
        conn.execute(
            "INSERT INTO knowledge_analysis
                (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
             VALUES (?1, 1, 1, 'fp', ?2, '')",
            params![id, status],
        )
        .unwrap();
    }
    assert!(conn
        .execute(
            "INSERT INTO knowledge_analysis
                (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
             VALUES (9, 1, 1, 'fp', 'published', '')",
            [],
        )
        .is_err());

    for kind in ["program", "test", "library_root"] {
        conn.execute(
            "INSERT INTO knowledge_entries
                (source_id, evidence_file_id, target_rel_path, kind, reason, evidence_hash)
             VALUES (1, 1, 'src/main.rs', ?1, 'bin', 'hash')",
            [kind],
        )
        .unwrap();
    }
    assert!(conn
        .execute(
            "INSERT INTO knowledge_entries
                (source_id, evidence_file_id, target_rel_path, kind, reason, evidence_hash)
             VALUES (1, 1, 'src/main.rs', 'script', 'bin', 'hash')",
            [],
        )
        .is_err());

    for status in ["resolved", "ambiguous", "unresolved"] {
        conn.execute(
            "INSERT INTO knowledge_resolutions (source_id, reference_id, status) VALUES (1, 1, ?1)",
            [status],
        )
        .unwrap();
    }
    assert!(conn
        .execute(
            "INSERT INTO knowledge_resolutions (source_id, reference_id, status) VALUES (1, 1, 'missing')",
            [],
        )
        .is_err());
}

#[test]
fn the_analysis_migration_uses_the_same_status_check() {
    let conn = crate::db::open_db(Path::new(":memory:")).unwrap();
    conn.execute_batch(
        "CREATE TABLE knowledge_analysis (
            source_id           INTEGER PRIMARY KEY,
            analysis_version    INTEGER NOT NULL,
            evidence_generation INTEGER NOT NULL,
            fingerprint         TEXT NOT NULL,
            status              TEXT NOT NULL CHECK (status IN ('complete', 'incomplete')),
            gaps                TEXT NOT NULL
        );",
    )
    .unwrap();
    apply_schema(&conn).unwrap();
    let sql = table_sql(&conn, "knowledge_analysis");
    assert!(sql.contains(ANALYSIS_STATUS_CHECK), "{sql}");
    assert!(!sql.contains("{analysis_status_check}"), "{sql}");
}
