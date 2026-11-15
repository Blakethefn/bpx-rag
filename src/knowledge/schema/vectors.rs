use anyhow::Result;
use rusqlite::Connection;

pub(super) fn vectors(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        &"        CREATE TABLE IF NOT EXISTS knowledge_embeddings (
            chunk_rowid INTEGER PRIMARY KEY,
            embedding   BLOB NOT NULL,
            dim         INTEGER NOT NULL DEFAULT 384,
            model_id    TEXT NOT NULL DEFAULT 'sentence-transformers/all-MiniLM-L6-v2'
        );

        CREATE TABLE IF NOT EXISTS knowledge_embedding_state (
            state_id   INTEGER PRIMARY KEY CHECK (state_id = 1),
            generation INTEGER NOT NULL DEFAULT 0,
            database_id TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS knowledge_embedding_changes (
            change_id INTEGER PRIMARY KEY AUTOINCREMENT,
            chunk_rowid INTEGER NOT NULL,
            model_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete'))
        );

",
    )?;
    Ok(())
}
