//! Knowledge database schema and connection management.
//!
//! The knowledge database is a single, global SQLite file at
//! `~/.local/share/bpx-rag/knowledge.db`.

use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::data_dir;

/// Returns the path to the knowledge database.
/// `~/.local/share/bpx-rag/knowledge.db`
pub fn knowledge_db_path() -> Result<PathBuf> {
    let dir = data_dir()?;
    crate::db::create_private_dir(&dir)?;
    Ok(dir.join("knowledge.db"))
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
            chunker_version INTEGER NOT NULL DEFAULT 1,
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

        CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_identifiers USING fts5(
            words,
            tokenize = 'porter unicode61'
        );

        CREATE TABLE IF NOT EXISTS knowledge_index_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
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

        CREATE TABLE IF NOT EXISTS knowledge_symbols (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id   INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            file_id     INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            kind        TEXT NOT NULL,
            container   TEXT,
            line_start  INTEGER NOT NULL,
            line_end    INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chunk_meta_source ON knowledge_chunk_meta(source_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_meta_file ON knowledge_chunk_meta(file_id);
        CREATE INDEX IF NOT EXISTS idx_files_source ON knowledge_files(source_id);
        CREATE INDEX IF NOT EXISTS idx_links_source ON knowledge_links(source_file_id);
        CREATE INDEX IF NOT EXISTS idx_links_target ON knowledge_links(target_file_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON knowledge_symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON knowledge_symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_source ON knowledge_symbols(source_id);

        -- Identifier-index maintenance. SQLite cannot put triggers on FTS5 tables, so the
        -- triggers sit on this ordinary table, which every chunk writer (including builds
        -- that predate the identifier index) inserts into and deletes from.
        CREATE TABLE IF NOT EXISTS knowledge_identifier_pending (
            chunk_rowid INTEGER PRIMARY KEY
        );
        CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_identifier_delete
        AFTER DELETE ON knowledge_chunk_meta BEGIN
            DELETE FROM knowledge_identifiers WHERE rowid = OLD.chunk_rowid;
            DELETE FROM knowledge_identifier_pending WHERE chunk_rowid = OLD.chunk_rowid;
        END;
        CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_identifier_insert
        AFTER INSERT ON knowledge_chunk_meta BEGIN
            INSERT OR IGNORE INTO knowledge_identifier_pending (chunk_rowid)
                VALUES (NEW.chunk_rowid);
        END;
        ",
    )?;
    ensure_chunker_version(conn)?;
    ensure_symbol_version(conn)?;
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

fn ensure_chunker_version(conn: &Connection) -> Result<()> {
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

/// Stage 7 metadata and watch controls. Rows indexed earlier keep NULL metadata until a sync
/// records it, and sources default to watch disabled.
fn ensure_freshness_and_watch_columns(conn: &Connection) -> Result<()> {
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

/// Files indexed before symbols existed have version 0. Sync backfills them.
///
/// `symbol_version` records the parser version the file's symbol rows were extracted with.
/// `symbols_hash` records the content hash those rows were extracted from. A writer that
/// predates symbols updates `content_hash` but never `symbols_hash`, so the mismatch marks
/// the rows stale. Symbol lookup then skips them until the next sync rebuilds them.
fn ensure_symbol_version(conn: &Connection) -> Result<()> {
    add_column_if_missing(
        conn,
        "knowledge_files",
        "symbol_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "knowledge_files", "symbols_hash", "TEXT")
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
mod tests;
