use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn ensure_analysis_status(conn: &Connection) -> Result<()> {
    let sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_analysis'",
        [],
        |row| row.get(0),
    )?;
    if sql.contains("'partial'") {
        return Ok(());
    }
    let foreign_keys: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    let migration = "
        BEGIN IMMEDIATE;
        CREATE TABLE knowledge_analysis_next (
            source_id           INTEGER PRIMARY KEY REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            analysis_version    INTEGER NOT NULL,
            evidence_generation INTEGER NOT NULL,
            fingerprint         TEXT NOT NULL,
            status              TEXT NOT NULL {analysis_status_check},
            gaps                TEXT NOT NULL
        );
        INSERT INTO knowledge_analysis_next
            (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
        SELECT source_id, analysis_version, evidence_generation, fingerprint, status, gaps
        FROM knowledge_analysis;
        DROP TABLE knowledge_analysis;
        ALTER TABLE knowledge_analysis_next RENAME TO knowledge_analysis;
        COMMIT;
        "
    .replace("{analysis_status_check}", super::ANALYSIS_STATUS_CHECK);
    let migrated = conn.execute_batch(&migration);
    if migrated.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    if foreign_keys != 0 {
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
    }
    migrated?;
    Ok(())
}

pub(super) fn ensure_embedding_state_row(conn: &Connection) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_embedding_state WHERE state_id = 1)",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute(
            "INSERT OR IGNORE INTO knowledge_embedding_state (state_id, generation, database_id)
             VALUES (1, 0, '')",
            [],
        )?;
    }
    Ok(())
}

pub(super) fn ensure_chunker_version(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare("PRAGMA table_info(knowledge_files)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|column| column == "chunker_version") {
        conn.execute(
            "ALTER TABLE knowledge_files ADD COLUMN chunker_version INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    Ok(())
}

pub(super) fn ensure_freshness_and_watch_columns(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "knowledge_files", "mtime_ns", "INTEGER")?;
    add_column_if_missing(conn, "knowledge_files", "stat_checked_ns", "INTEGER")?;
    add_column_if_missing(
        conn,
        "knowledge_sources",
        "watch_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|existing| existing == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

pub(super) fn ensure_symbol_version(conn: &Connection) -> Result<()> {
    add_column_if_missing(
        conn,
        "knowledge_files",
        "symbol_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "knowledge_files", "symbols_hash", "TEXT")?;
    add_column_if_missing(conn, "knowledge_files", "symbol_status", "TEXT")?;
    add_column_if_missing(conn, "knowledge_symbols", "visibility", "TEXT")
}

pub(super) fn ensure_reference_columns(conn: &Connection) -> Result<()> {
    add_column_if_missing(
        conn,
        "knowledge_files",
        "reference_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "knowledge_files", "references_hash", "TEXT")?;
    add_column_if_missing(conn, "knowledge_files", "reference_status", "TEXT")
}

pub(super) fn ensure_database_instance_id(conn: &Connection) -> Result<()> {
    let id: String = conn.query_row(
        "SELECT database_id FROM knowledge_embedding_state WHERE state_id = 1",
        [],
        |row| row.get(0),
    )?;
    if id.is_empty() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seed = format!(
            "{nanos}:{}:{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let instance_id = format!("{:x}", Sha256::digest(seed.as_bytes()));
        conn.execute(
            "UPDATE knowledge_embedding_state SET database_id = ?1 WHERE state_id = 1",
            [&instance_id],
        )?;
    }
    Ok(())
}

pub(super) fn ensure_embedding_model_id(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare("PRAGMA table_info(knowledge_embeddings)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|column| column == "model_id") {
        conn.execute(
            "ALTER TABLE knowledge_embeddings ADD COLUMN model_id TEXT NOT NULL
             DEFAULT 'sentence-transformers/all-MiniLM-L6-v2'",
            [],
        )?;
    }
    Ok(())
}
