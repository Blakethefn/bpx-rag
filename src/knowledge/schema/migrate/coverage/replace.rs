use anyhow::Result;
use rusqlite::Connection;

pub(super) fn install(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_prepare_replace
         BEFORE INSERT ON knowledge_embeddings
         WHEN EXISTS (SELECT 1 FROM knowledge_embeddings
                      WHERE chunk_rowid = NEW.chunk_rowid) BEGIN
            INSERT INTO knowledge_embedding_coverage_replace
                (chunk_rowid, file_id, model_id)
            SELECT e.chunk_rowid, kcm.file_id, e.model_id
            FROM knowledge_embeddings e
            LEFT JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = e.chunk_rowid
            WHERE e.chunk_rowid = NEW.chunk_rowid
            ON CONFLICT(chunk_rowid) DO UPDATE
                SET file_id = excluded.file_id, model_id = excluded.model_id;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_replace_cleanup
         AFTER UPDATE ON knowledge_embeddings BEGIN
            DELETE FROM knowledge_embedding_coverage_replace
            WHERE chunk_rowid = NEW.chunk_rowid;
         END;",
    )?;
    Ok(())
}
