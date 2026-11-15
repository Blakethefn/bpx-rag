use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

use crate::db::data_dir;

mod catalog;
mod graph;
mod migrate;
mod vectors;
use migrate::{
    ensure_analysis_status, ensure_chunker_version, ensure_database_instance_id,
    ensure_embedding_model_id, ensure_embedding_state_row, ensure_freshness_and_watch_columns,
    ensure_reference_columns, ensure_symbol_version,
};

pub(super) const ANALYSIS_STATUS_CHECK: &str =
    "CHECK (status IN ('complete', 'partial', 'incomplete'))";

pub(super) fn check_in(column: &str, values: &[&str]) -> String {
    let list = values
        .iter()
        .map(|value| format!("'{value}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CHECK ({column} IN ({list}))")
}

pub fn knowledge_db_path() -> Result<PathBuf> {
    let dir = data_dir()?;
    crate::db::create_private_dir(&dir)?;
    Ok(dir.join("knowledge.db"))
}

pub(crate) fn apply_schema(conn: &Connection) -> Result<()> {
    ensure_schema(conn)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    catalog::catalog(conn)?;
    vectors::vectors(conn)?;
    graph::graph(conn)?;
    ensure_analysis_status(conn)?;
    ensure_embedding_state_row(conn)?;
    ensure_chunker_version(conn)?;
    ensure_symbol_version(conn)?;
    ensure_reference_columns(conn)?;
    ensure_freshness_and_watch_columns(conn)?;
    ensure_embedding_model_id(conn)?;
    ensure_database_instance_id(conn)?;
    crate::knowledge::identifiers::ensure_identifier_index(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_embeddings_model ON knowledge_embeddings(model_id);
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_insert
         AFTER INSERT ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_update
         AFTER UPDATE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_delete
         AFTER DELETE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes (chunk_rowid, model_id, operation)
                VALUES (OLD.chunk_rowid, OLD.model_id, 'delete');
            UPDATE knowledge_embedding_state SET generation = last_insert_rowid() WHERE state_id = 1;
         END;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
