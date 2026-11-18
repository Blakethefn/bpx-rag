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
    migrate::ensure_file_extension(conn)?;
    graph::graph(conn)?;
    ensure_analysis_status(conn)?;
    ensure_embedding_state_row(conn)?;
    ensure_chunker_version(conn)?;
    ensure_symbol_version(conn)?;
    ensure_reference_columns(conn)?;
    ensure_freshness_and_watch_columns(conn)?;
    ensure_embedding_model_id(conn)?;
    ensure_database_instance_id(conn)?;
    migrate::ensure_embedding_change_tracking(conn)?;
    migrate::ensure_embedding_coverage(conn)?;
    migrate::ensure_source_stats(conn)?;
    crate::knowledge::identifiers::ensure_identifier_index(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_embeddings_model ON knowledge_embeddings(model_id);",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
