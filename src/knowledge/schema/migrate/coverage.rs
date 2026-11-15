use anyhow::Result;
use rusqlite::Connection;

mod ready;

const MARKER: &str = "embedding_coverage_v1";
const VERSION: &str = "1";

pub(in crate::knowledge::schema) fn ensure_embedding_coverage(conn: &Connection) -> Result<()> {
    if ready::coverage_is_current(conn)? {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        let coverage_is_current = ready::coverage_is_current(conn)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge_embedding_coverage (
                file_id INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
                model_id TEXT NOT NULL,
                embedded_count INTEGER NOT NULL CHECK (embedded_count >= 0),
                PRIMARY KEY (file_id, model_id)
            );
            CREATE INDEX IF NOT EXISTS idx_embedding_coverage_model_file
                ON knowledge_embedding_coverage(model_id, file_id);",
        )?;
        if !coverage_is_current {
            drop_coverage_triggers(conn)?;
            conn.execute("DELETE FROM knowledge_embedding_coverage", [])?;
            conn.execute_batch(
                "INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
                 SELECT kcm.file_id, ke.model_id, COUNT(*)
                 FROM knowledge_embeddings ke
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                 GROUP BY kcm.file_id, ke.model_id;",
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO knowledge_index_meta(key, value) VALUES (?1, ?2)",
                [MARKER, VERSION],
            )?;
        }
        install_coverage_triggers(conn)?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

fn drop_coverage_triggers(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_insert;
         DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_update;
         DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_delete;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_insert;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_delete;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_move;",
    )?;
    Ok(())
}

fn install_coverage_triggers(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_insert
         AFTER INSERT ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_coverage
                (file_id, model_id, embedded_count)
            SELECT file_id, NEW.model_id, 1 FROM knowledge_chunk_meta
            WHERE chunk_rowid = NEW.chunk_rowid
            ON CONFLICT(file_id, model_id) DO UPDATE
                SET embedded_count = embedded_count + 1;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_update
         AFTER UPDATE OF chunk_rowid, model_id ON knowledge_embeddings
         WHEN OLD.chunk_rowid <> NEW.chunk_rowid OR OLD.model_id <> NEW.model_id BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - 1
            WHERE file_id = (SELECT file_id FROM knowledge_chunk_meta
                             WHERE chunk_rowid = OLD.chunk_rowid)
              AND model_id = OLD.model_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
            INSERT OR IGNORE INTO knowledge_embedding_coverage
                (file_id, model_id, embedded_count)
            SELECT file_id, NEW.model_id, 0 FROM knowledge_chunk_meta
            WHERE chunk_rowid = NEW.chunk_rowid;
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + 1
            WHERE file_id = (SELECT file_id FROM knowledge_chunk_meta
                             WHERE chunk_rowid = NEW.chunk_rowid)
              AND model_id = NEW.model_id;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_delete
         AFTER DELETE ON knowledge_embeddings BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - 1
            WHERE file_id = (SELECT file_id FROM knowledge_chunk_meta
                             WHERE chunk_rowid = OLD.chunk_rowid)
              AND model_id = OLD.model_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_coverage_insert
         AFTER INSERT ON knowledge_chunk_meta
         WHEN EXISTS (SELECT 1 FROM knowledge_embeddings
                      WHERE chunk_rowid = NEW.chunk_rowid) BEGIN
            INSERT INTO knowledge_embedding_coverage
                (file_id, model_id, embedded_count)
            SELECT NEW.file_id, model_id, COUNT(*) FROM knowledge_embeddings
            WHERE chunk_rowid = NEW.chunk_rowid
            GROUP BY model_id
            ON CONFLICT(file_id, model_id) DO UPDATE
                SET embedded_count = embedded_count + excluded.embedded_count;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_coverage_delete
         AFTER DELETE ON knowledge_chunk_meta
         WHEN EXISTS (SELECT 1 FROM knowledge_embeddings
                      WHERE chunk_rowid = OLD.chunk_rowid) BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = OLD.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = OLD.file_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_coverage_move
         AFTER UPDATE OF file_id ON knowledge_chunk_meta
         WHEN OLD.file_id <> NEW.file_id
           AND EXISTS (SELECT 1 FROM knowledge_embeddings
                       WHERE chunk_rowid = NEW.chunk_rowid) BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = NEW.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = OLD.file_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
            INSERT OR IGNORE INTO knowledge_embedding_coverage
                (file_id, model_id, embedded_count)
            SELECT NEW.file_id, model_id, 0 FROM knowledge_embeddings
            WHERE chunk_rowid = NEW.chunk_rowid;
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = NEW.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = NEW.file_id;
         END;",
    )?;
    Ok(())
}
