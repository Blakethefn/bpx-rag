use super::super::*;
use std::path::Path;

#[test]
fn an_existing_analysis_check_accepts_partial() {
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
    conn.execute(
        "INSERT INTO knowledge_sources (label, path) VALUES ('fresh', '/tmp/fresh')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge_analysis
            (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
         VALUES (?1, 3, 1, 'abc', 'partial', '')",
        [conn.last_insert_rowid()],
    )
    .unwrap();

    let conn = crate::db::open_db(Path::new(":memory:")).unwrap();
    apply_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO knowledge_sources (label, path) VALUES ('demo', '/tmp/demo')",
        [],
    )
    .unwrap();
    let source_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_analysis
            (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
         VALUES (?1, 2, 4, 'kept', 'incomplete', 'broken.rs:parse_error')",
        [source_id],
    )
    .unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         BEGIN IMMEDIATE;
         CREATE TABLE knowledge_analysis_old (
            source_id           INTEGER PRIMARY KEY,
            analysis_version    INTEGER NOT NULL,
            evidence_generation INTEGER NOT NULL,
            fingerprint         TEXT NOT NULL,
            status              TEXT NOT NULL CHECK (status IN ('complete', 'incomplete')),
            gaps                TEXT NOT NULL
         );
         INSERT INTO knowledge_analysis_old
            SELECT source_id, analysis_version, evidence_generation, fingerprint, status, gaps
            FROM knowledge_analysis;
         DROP TABLE knowledge_analysis;
         ALTER TABLE knowledge_analysis_old RENAME TO knowledge_analysis;
         COMMIT;
         PRAGMA foreign_keys = ON;",
    )
    .unwrap();

    apply_schema(&conn).unwrap();
    let (status, gaps): (String, String) = conn
        .query_row(
            "SELECT status, gaps FROM knowledge_analysis WHERE source_id = ?1",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "incomplete");
    assert_eq!(gaps, "broken.rs:parse_error");
    conn.execute(
        "UPDATE knowledge_analysis SET status = 'partial' WHERE source_id = ?1",
        [source_id],
    )
    .unwrap();
    apply_schema(&conn).unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM knowledge_analysis WHERE source_id = ?1",
            [source_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "partial");
}
