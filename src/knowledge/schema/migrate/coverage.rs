use anyhow::Result;
use rusqlite::Connection;

mod ready;
mod replace;
mod triggers;

const MARKER: &str = "embedding_coverage_v1";
const VERSION: &str = "3";

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
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge_embedding_coverage_replace (
                chunk_rowid INTEGER PRIMARY KEY,
                file_id INTEGER,
                model_id TEXT NOT NULL
            );",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge_chunk_meta_coverage_replace (
                chunk_rowid INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL
            );",
        )?;
        if !coverage_is_current {
            drop_coverage_triggers(conn)?;
            conn.execute("DELETE FROM knowledge_embedding_coverage_replace", [])?;
            conn.execute("DELETE FROM knowledge_chunk_meta_coverage_replace", [])?;
            conn.execute("DELETE FROM knowledge_embedding_coverage", [])?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_embeddings_model
                 ON knowledge_embeddings(model_id);",
            )?;
            conn.execute_batch(
                "INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
                 SELECT kcm.file_id, ke.model_id, COUNT(*)
                 FROM knowledge_embeddings ke INDEXED BY idx_embeddings_model
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
                 GROUP BY kcm.file_id, ke.model_id;",
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO knowledge_index_meta(key, value) VALUES (?1, ?2)",
                [MARKER, VERSION],
            )?;
        }
        triggers::install(conn)?;
        replace::install(conn)?;
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
         DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_prepare_replace;
         DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_replace_cleanup;
         DROP TRIGGER IF EXISTS knowledge_embeddings_coverage_chunkrowid_immutable;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_prepare_replace;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_replace_cleanup;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_chunkrowid_immutable;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_insert;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_delete;
         DROP TRIGGER IF EXISTS knowledge_chunk_meta_coverage_move;",
    )?;
    Ok(())
}
