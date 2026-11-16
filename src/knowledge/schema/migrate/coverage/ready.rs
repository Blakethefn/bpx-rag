use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

const MARKER: &str = "embedding_coverage_v1";
const VERSION: &str = "2";
const TRIGGERS: [&str; 8] = [
    "knowledge_embeddings_coverage_insert",
    "knowledge_embeddings_coverage_update",
    "knowledge_embeddings_coverage_delete",
    "knowledge_embeddings_coverage_prepare_replace",
    "knowledge_embeddings_coverage_replace_cleanup",
    "knowledge_chunk_meta_coverage_insert",
    "knowledge_chunk_meta_coverage_delete",
    "knowledge_chunk_meta_coverage_move",
];

pub(super) fn coverage_is_current(conn: &Connection) -> Result<bool> {
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'knowledge_embedding_coverage')",
        [],
        |row| row.get(0),
    )?;
    let replace_table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'knowledge_embedding_coverage_replace')",
        [],
        |row| row.get(0),
    )?;
    if !table_exists || !replace_table_exists {
        return Ok(false);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key = ?1",
            [MARKER],
            |row| row.get(0),
        )
        .optional()?;
    if version.as_deref() != Some(VERSION) {
        return Ok(false);
    }
    for name in TRIGGERS {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master
             WHERE type = 'trigger' AND name = ?1)",
            [name],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(false);
        }
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'index' AND name = 'idx_embedding_coverage_model_file')",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}
