use anyhow::Result;
use rusqlite::Connection;

pub(super) fn catalog(conn: &Connection) -> Result<()> {
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

",
    )?;
    Ok(())
}
