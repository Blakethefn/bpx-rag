//! Knowledge database schema and connection management.
//!
//! The knowledge database is a single, global SQLite file at
//! `~/.local/share/bpcontext/knowledge.db`. It is separate from the per-project
//! session and content databases.

use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{data_dir, open_db};
use crate::search_memory;

/// Returns the path to the knowledge database.
/// `~/.local/share/bpcontext/knowledge.db`
pub fn knowledge_db_path() -> Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("knowledge.db"))
}

/// Opens the knowledge database, creating it and running schema DDL if needed.
/// Uses the same connection settings as `db::open_db` (WAL, FK, busy timeout).
pub fn open_knowledge_db() -> Result<Connection> {
    let path = knowledge_db_path()?;
    let conn = open_db(&path)?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// Runs schema DDL. Called by `open_knowledge_db` on every open.
/// All statements use `IF NOT EXISTS` so repeated calls are idempotent.
///
/// Exposed as `pub(crate)` so tests can apply the schema to in-memory connections.
pub(crate) fn apply_schema(conn: &Connection) -> Result<()> {
    ensure_schema(conn)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS knowledge_sources (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            label       TEXT NOT NULL UNIQUE,
            path        TEXT NOT NULL,
            glob        TEXT,
            enrichments TEXT NOT NULL DEFAULT '[]',
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            last_sync   TEXT,
            file_count  INTEGER NOT NULL DEFAULT 0,
            chunk_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS knowledge_files (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id    INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            rel_path     TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            size_bytes   INTEGER NOT NULL,
            indexed_at   TEXT NOT NULL DEFAULT (datetime('now')),
            chunk_count  INTEGER NOT NULL DEFAULT 0,
            UNIQUE(source_id, rel_path)
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks USING fts5(
            title,
            content,
            tokenize = 'porter unicode61'
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_trigram USING fts5(
            title,
            content,
            tokenize = 'trigram'
        );

        CREATE TABLE IF NOT EXISTS knowledge_chunk_meta (
            chunk_rowid INTEGER PRIMARY KEY,
            file_id     INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            source_id   INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            line_start  INTEGER,
            line_end    INTEGER,
            metadata    TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE IF NOT EXISTS knowledge_embeddings (
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
        INSERT OR IGNORE INTO knowledge_embedding_state (state_id, generation, database_id)
            VALUES (1, 0, '');

        CREATE TABLE IF NOT EXISTS knowledge_embedding_changes (
            change_id INTEGER PRIMARY KEY AUTOINCREMENT,
            chunk_rowid INTEGER NOT NULL,
            model_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete'))
        );

        CREATE TABLE IF NOT EXISTS knowledge_links (
            source_file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            target_file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            link_type      TEXT NOT NULL,
            confidence     REAL NOT NULL DEFAULT 1.0,
            PRIMARY KEY (source_file_id, target_file_id, link_type)
        );

        CREATE INDEX IF NOT EXISTS idx_chunk_meta_source ON knowledge_chunk_meta(source_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_meta_file ON knowledge_chunk_meta(file_id);
        CREATE INDEX IF NOT EXISTS idx_files_source ON knowledge_files(source_id);
        CREATE INDEX IF NOT EXISTS idx_links_source ON knowledge_links(source_file_id);
        CREATE INDEX IF NOT EXISTS idx_links_target ON knowledge_links(target_file_id);
        ",
    )?;
    ensure_embedding_model_id(conn)?;
    ensure_database_instance_id(conn)?;
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
    search_memory::init_knowledge_schema(conn)?;
    Ok(())
}

fn ensure_database_instance_id(conn: &Connection) -> Result<()> {
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

fn ensure_embedding_model_id(conn: &Connection) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_embedding_rows_are_migrated_to_their_legacy_model_identity() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE knowledge_embeddings (
                chunk_rowid INTEGER PRIMARY KEY,
                embedding BLOB NOT NULL,
                dim INTEGER NOT NULL DEFAULT 384
             );
             INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
             VALUES (1, X'00000000', 384);",
        )
        .unwrap();

        apply_schema(&conn).unwrap();
        apply_schema(&conn).unwrap();

        let model_id: String = conn
            .query_row(
                "SELECT model_id FROM knowledge_embeddings WHERE chunk_rowid = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model_id, "sentence-transformers/all-MiniLM-L6-v2");
    }

    #[test]
    fn embedding_insert_update_and_delete_advance_ann_generation() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();
        let generation = || {
            conn.query_row(
                "SELECT generation FROM knowledge_embedding_state WHERE state_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
        };

        assert_eq!(generation(), 0);
        conn.execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
             VALUES (1, X'00000000', 1)",
            [],
        )
        .unwrap();
        assert_eq!(generation(), 1);
        conn.execute(
            "UPDATE knowledge_embeddings SET model_id = 'jinaai/jina-embeddings-v2-base-code'
             WHERE chunk_rowid = 1",
            [],
        )
        .unwrap();
        assert_eq!(generation(), 2);
        conn.execute("DELETE FROM knowledge_embeddings WHERE chunk_rowid = 1", [])
            .unwrap();
        assert_eq!(generation(), 3);
    }
}
