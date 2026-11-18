use super::*;
use rusqlite::Connection;

#[test]
fn source_status_aggregation_uses_compact_indexes() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    crate::knowledge::schema::apply_schema(&conn).unwrap();
    let mut statement = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {STATUS_COUNTS}"))
        .unwrap();
    let plan = statement
        .query_map(["model-a"], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH st USING INTEGER PRIMARY KEY"),
        "{plan}"
    );
    assert!(
        plan.contains("USING COVERING INDEX idx_embedding_coverage_status"),
        "{plan}"
    );
    assert!(!plan.contains("knowledge_chunks"), "{plan}");
    assert!(!plan.contains("knowledge_embeddings"), "{plan}");
    assert!(!plan.contains("chunk_payloads"), "{plan}");
}
