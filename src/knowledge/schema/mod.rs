//! Knowledge database schema and connection management.
//!
//! The knowledge database is a single, global SQLite file at
//! `~/.local/share/bpx-rag/knowledge.db`.

use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

use crate::db::data_dir;
use crate::knowledge::resolution::{ENTRY_KINDS, RESOLUTION_STATUSES};

mod migrate;
use migrate::{
    ensure_analysis_status, ensure_chunker_version, ensure_database_instance_id,
    ensure_embedding_model_id, ensure_embedding_state_row, ensure_freshness_and_watch_columns,
    ensure_reference_columns, ensure_symbol_version,
};

/// `knowledge_analysis.status`. Fresh creates and the partial migration share this check.
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
        &"
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
            line_end    INTEGER NOT NULL,
            visibility  TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_chunk_meta_source ON knowledge_chunk_meta(source_id);
        CREATE INDEX IF NOT EXISTS idx_chunk_meta_file ON knowledge_chunk_meta(file_id);
        CREATE INDEX IF NOT EXISTS idx_files_source ON knowledge_files(source_id);
        CREATE INDEX IF NOT EXISTS idx_links_source ON knowledge_links(source_file_id);
        CREATE INDEX IF NOT EXISTS idx_links_target ON knowledge_links(target_file_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON knowledge_symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON knowledge_symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_source ON knowledge_symbols(source_id);

        CREATE TABLE IF NOT EXISTS knowledge_references (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id   INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            file_id     INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            line_start  INTEGER NOT NULL,
            line_end    INTEGER NOT NULL,
            kind        TEXT NOT NULL CHECK (kind IN ('name', 'import', 'module', 'path', 'type')),
            specifier   TEXT NOT NULL,
            qualifier   TEXT
        );

        CREATE TABLE IF NOT EXISTS knowledge_entries (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id        INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            evidence_file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            target_file_id   INTEGER REFERENCES knowledge_files(id) ON DELETE SET NULL,
            target_rel_path  TEXT NOT NULL,
            kind             TEXT NOT NULL {entry_kind_check},
            reason           TEXT NOT NULL,
            evidence_hash    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS knowledge_resolutions (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id        INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            reference_id     INTEGER NOT NULL REFERENCES knowledge_references(id) ON DELETE CASCADE,
            status           TEXT NOT NULL {resolution_status_check},
            target_symbol_id INTEGER REFERENCES knowledge_symbols(id) ON DELETE CASCADE,
            target_file_id   INTEGER REFERENCES knowledge_files(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS knowledge_analysis (
            source_id           INTEGER PRIMARY KEY REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            analysis_version    INTEGER NOT NULL,
            evidence_generation INTEGER NOT NULL,
            fingerprint         TEXT NOT NULL,
            status              TEXT NOT NULL {analysis_status_check},
            gaps                TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS knowledge_findings (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id           INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            kind                TEXT NOT NULL CHECK (kind IN ('possibly_unused_symbol', 'unreachable_file')),
            file_id             INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
            symbol_id           INTEGER REFERENCES knowledge_symbols(id) ON DELETE CASCADE,
            reason              TEXT NOT NULL,
            analysis_version    INTEGER NOT NULL,
            evidence_generation INTEGER NOT NULL,
            fingerprint         TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_references_file ON knowledge_references(file_id);
        CREATE INDEX IF NOT EXISTS idx_references_source ON knowledge_references(source_id);
        CREATE INDEX IF NOT EXISTS idx_entries_source ON knowledge_entries(source_id);
        CREATE INDEX IF NOT EXISTS idx_entries_evidence ON knowledge_entries(evidence_file_id);
        CREATE INDEX IF NOT EXISTS idx_entries_target ON knowledge_entries(target_file_id);
        CREATE INDEX IF NOT EXISTS idx_resolutions_reference ON knowledge_resolutions(reference_id);
        CREATE INDEX IF NOT EXISTS idx_resolutions_symbol ON knowledge_resolutions(target_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_findings_source ON knowledge_findings(source_id);

        -- Vocabulary for fuzzy correction. Created here, not on the search path, so searching
        -- never writes to the database.
        CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab USING fts5vocab(knowledge_chunks, instance);

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
        "
        .replace("{entry_kind_check}", &check_in("kind", ENTRY_KINDS))
        .replace(
            "{resolution_status_check}",
            &check_in("status", RESOLUTION_STATUSES),
        )
        .replace("{analysis_status_check}", ANALYSIS_STATUS_CHECK),
    )?;
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
