use anyhow::Result;
use rusqlite::Connection;

use crate::knowledge::resolution::{ENTRY_KINDS, RESOLUTION_STATUSES};

pub(super) fn graph(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        &"        CREATE TABLE IF NOT EXISTS knowledge_links (
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
        CREATE INDEX IF NOT EXISTS idx_files_rel_path ON knowledge_files(rel_path);
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
        .replace("{entry_kind_check}", &super::check_in("kind", ENTRY_KINDS))
        .replace(
            "{resolution_status_check}",
            &super::check_in("status", RESOLUTION_STATUSES),
        )
        .replace("{analysis_status_check}", super::ANALYSIS_STATUS_CHECK),
    )?;
    Ok(())
}
