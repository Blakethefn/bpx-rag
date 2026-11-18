use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

const MARKER: &str = "source_stats_v1";

pub(in crate::knowledge::schema) fn ensure_source_stats(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS source_stats (
            source_id INTEGER PRIMARY KEY REFERENCES knowledge_sources(id) ON DELETE CASCADE,
            chunk_count INTEGER NOT NULL DEFAULT 0 CHECK (chunk_count >= 0),
            embedded_count INTEGER NOT NULL DEFAULT 0 CHECK (embedded_count >= 0),
            total_bytes INTEGER NOT NULL DEFAULT 0 CHECK (total_bytes >= 0),
            last_indexed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_embedding_coverage_status
            ON knowledge_embedding_coverage(file_id, model_id, embedded_count);
        CREATE TABLE IF NOT EXISTS source_stats_meta_replace (
            chunk_rowid INTEGER PRIMARY KEY,
            source_id INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS source_stats_embedding_replace (
            chunk_rowid INTEGER PRIMARY KEY
        );",
    )?;
    let current: Option<String> = conn
        .query_row(
            "SELECT value FROM knowledge_index_meta WHERE key = ?1",
            [MARKER],
            |row| row.get(0),
        )
        .optional()?;
    if current.as_deref() != Some("1") {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            conn.execute_batch(
                "DELETE FROM source_stats;
                 INSERT INTO source_stats(source_id, chunk_count, embedded_count, total_bytes, last_indexed_at)
                 SELECT s.id,
                    COALESCE(c.chunk_count, 0),
                    COALESCE(e.embedded_count, 0),
                    COALESCE(f.total_bytes, 0),
                    f.last_indexed_at
                 FROM knowledge_sources s
                 LEFT JOIN (
                    SELECT source_id, COUNT(*) AS chunk_count
                    FROM knowledge_chunk_meta GROUP BY source_id
                 ) c ON c.source_id = s.id
                 LEFT JOIN (
                    SELECT f.source_id, SUM(cov.embedded_count) AS embedded_count
                    FROM knowledge_embedding_coverage cov
                    JOIN knowledge_files f ON f.id = cov.file_id
                    GROUP BY f.source_id
                 ) e ON e.source_id = s.id
                 LEFT JOIN (
                    SELECT source_id, SUM(size_bytes) AS total_bytes, MAX(indexed_at) AS last_indexed_at
                    FROM knowledge_files GROUP BY source_id
                 ) f ON f.source_id = s.id;",
            )?;
            install(conn)?;
            conn.execute(
                "INSERT OR REPLACE INTO knowledge_index_meta(key, value) VALUES (?1, '1')",
                [MARKER],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    } else {
        install(conn)?;
    }
    Ok(())
}

/// Installs the source_stats triggers.
///
/// Do not switch the knowledge_chunk_meta or knowledge_embeddings writes to
/// INSERT OR REPLACE. The replace branch recounts every chunk in the source
/// per replaced row, so a bulk replace becomes O(n^2).
fn install(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS source_stats_source_insert
         AFTER INSERT ON knowledge_sources BEGIN
            INSERT OR IGNORE INTO source_stats(source_id) VALUES (NEW.id);
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_file_insert
         AFTER INSERT ON knowledge_files BEGIN
            INSERT OR IGNORE INTO source_stats(source_id) VALUES (NEW.source_id);
            UPDATE source_stats SET total_bytes = total_bytes + NEW.size_bytes,
                last_indexed_at = MAX(COALESCE(last_indexed_at, ''), NEW.indexed_at)
            WHERE source_id = NEW.source_id;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_file_update
         AFTER UPDATE OF source_id, size_bytes, indexed_at ON knowledge_files
         WHEN OLD.source_id <> NEW.source_id OR OLD.size_bytes <> NEW.size_bytes
              OR OLD.indexed_at <> NEW.indexed_at BEGIN
            INSERT OR IGNORE INTO source_stats(source_id) VALUES (NEW.source_id);
            UPDATE source_stats SET total_bytes = total_bytes
                    + CASE WHEN source_id = NEW.source_id THEN NEW.size_bytes ELSE 0 END
                    - CASE WHEN source_id = OLD.source_id THEN OLD.size_bytes ELSE 0 END,
                last_indexed_at = (SELECT MAX(indexed_at) FROM knowledge_files
                                   WHERE source_id = source_stats.source_id)
            WHERE source_id IN (OLD.source_id, NEW.source_id);
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_file_delete
         AFTER DELETE ON knowledge_files BEGIN
            UPDATE source_stats SET total_bytes = total_bytes - OLD.size_bytes,
                last_indexed_at = (SELECT MAX(indexed_at) FROM knowledge_files WHERE source_id = OLD.source_id)
                WHERE source_id = OLD.source_id;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_meta_prepare_replace
         BEFORE INSERT ON knowledge_chunk_meta
         WHEN EXISTS (SELECT 1 FROM knowledge_chunk_meta WHERE chunk_rowid = NEW.chunk_rowid) BEGIN
            INSERT INTO source_stats_meta_replace(chunk_rowid, source_id)
            SELECT chunk_rowid, source_id FROM knowledge_chunk_meta WHERE chunk_rowid = NEW.chunk_rowid
            ON CONFLICT(chunk_rowid) DO UPDATE SET source_id = excluded.source_id;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_meta_insert
         AFTER INSERT ON knowledge_chunk_meta BEGIN
            INSERT OR IGNORE INTO source_stats(source_id) VALUES (NEW.source_id);
            UPDATE source_stats SET chunk_count = chunk_count + 1,
                embedded_count = embedded_count + EXISTS(
                    SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = NEW.chunk_rowid)
            WHERE source_id = NEW.source_id
              AND NOT EXISTS (SELECT 1 FROM source_stats_meta_replace WHERE chunk_rowid = NEW.chunk_rowid);
            UPDATE source_stats SET
                chunk_count = (SELECT COUNT(*) FROM knowledge_chunk_meta
                               WHERE source_id = source_stats.source_id),
                embedded_count = (SELECT COUNT(*) FROM knowledge_embeddings e
                                  JOIN knowledge_chunk_meta m ON m.chunk_rowid = e.chunk_rowid
                                  WHERE m.source_id = source_stats.source_id),
                total_bytes = COALESCE((SELECT SUM(size_bytes) FROM knowledge_files
                                        WHERE source_id = source_stats.source_id), 0),
                last_indexed_at = (SELECT MAX(indexed_at) FROM knowledge_files
                                   WHERE source_id = source_stats.source_id)
            WHERE EXISTS (SELECT 1 FROM source_stats_meta_replace
                          WHERE chunk_rowid = NEW.chunk_rowid)
              AND (source_id = NEW.source_id
                   OR source_id = (SELECT source_id FROM source_stats_meta_replace
                                   WHERE chunk_rowid = NEW.chunk_rowid));
            DELETE FROM source_stats_meta_replace WHERE chunk_rowid = NEW.chunk_rowid;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_meta_delete
         AFTER DELETE ON knowledge_chunk_meta BEGIN
            UPDATE source_stats SET chunk_count = chunk_count - 1,
                embedded_count = embedded_count - EXISTS(
                    SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = OLD.chunk_rowid)
            WHERE source_id = OLD.source_id
              AND NOT EXISTS (SELECT 1 FROM source_stats_meta_replace WHERE chunk_rowid = OLD.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM knowledge_chunk_meta WHERE chunk_rowid = OLD.chunk_rowid);
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_meta_move
         AFTER UPDATE OF source_id ON knowledge_chunk_meta
         WHEN OLD.source_id <> NEW.source_id BEGIN
            UPDATE source_stats SET chunk_count = chunk_count - 1,
                embedded_count = embedded_count - EXISTS(
                    SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = NEW.chunk_rowid)
            WHERE source_id = OLD.source_id;
            INSERT OR IGNORE INTO source_stats(source_id) VALUES (NEW.source_id);
            UPDATE source_stats SET chunk_count = chunk_count + 1,
                embedded_count = embedded_count + EXISTS(
                    SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = NEW.chunk_rowid)
            WHERE source_id = NEW.source_id;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_embedding_prepare_replace
         BEFORE INSERT ON knowledge_embeddings
         WHEN EXISTS (SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = NEW.chunk_rowid) BEGIN
            INSERT OR IGNORE INTO source_stats_embedding_replace(chunk_rowid) VALUES (NEW.chunk_rowid);
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_embedding_insert
         AFTER INSERT ON knowledge_embeddings BEGIN
            UPDATE source_stats SET embedded_count = embedded_count + 1
            WHERE source_id = (SELECT source_id FROM knowledge_chunk_meta WHERE chunk_rowid = NEW.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM source_stats_embedding_replace WHERE chunk_rowid = NEW.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM knowledge_embeddings
                              WHERE chunk_rowid = NEW.chunk_rowid AND rowid <> NEW.rowid);
            DELETE FROM source_stats_embedding_replace WHERE chunk_rowid = NEW.chunk_rowid;
         END;
         CREATE TRIGGER IF NOT EXISTS source_stats_embedding_delete
         AFTER DELETE ON knowledge_embeddings BEGIN
            UPDATE source_stats SET embedded_count = embedded_count - 1
            WHERE source_id = (SELECT source_id FROM knowledge_chunk_meta WHERE chunk_rowid = OLD.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM knowledge_embeddings WHERE chunk_rowid = OLD.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM source_stats_embedding_replace
                              WHERE chunk_rowid = OLD.chunk_rowid);
         END;",
    )?;
    Ok(())
}
