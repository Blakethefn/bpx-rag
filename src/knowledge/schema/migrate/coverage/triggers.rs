use anyhow::Result;
use rusqlite::Connection;

pub(super) fn install(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_insert
         AFTER INSERT ON knowledge_embeddings BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + 1
            WHERE model_id = NEW.model_id
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage_replace
                              WHERE chunk_rowid = NEW.chunk_rowid)
              AND EXISTS (SELECT 1 FROM knowledge_chunk_meta kcm
                          WHERE kcm.chunk_rowid = NEW.chunk_rowid
                            AND kcm.file_id = knowledge_embedding_coverage.file_id);
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT kcm.file_id, NEW.model_id, 1 FROM knowledge_chunk_meta kcm
            WHERE kcm.chunk_rowid = NEW.chunk_rowid
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage_replace
                              WHERE chunk_rowid = NEW.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage c
                              WHERE c.file_id = kcm.file_id AND c.model_id = NEW.model_id);
            DELETE FROM knowledge_embedding_coverage
            WHERE EXISTS (SELECT 1 FROM knowledge_embedding_coverage_replace
                          WHERE chunk_rowid = NEW.chunk_rowid)
              AND ((file_id, model_id) IN (
                    SELECT file_id, model_id FROM knowledge_embedding_coverage_replace
                    WHERE chunk_rowid = NEW.chunk_rowid
                  ) OR (file_id = (SELECT file_id FROM knowledge_chunk_meta
                                   WHERE chunk_rowid = NEW.chunk_rowid)
                       AND model_id = NEW.model_id));
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT kcm.file_id, ke.model_id, COUNT(*)
            FROM knowledge_chunk_meta kcm
            JOIN knowledge_embeddings ke ON ke.chunk_rowid = kcm.chunk_rowid
            WHERE EXISTS (SELECT 1 FROM knowledge_embedding_coverage_replace
                          WHERE chunk_rowid = NEW.chunk_rowid)
              AND ((kcm.file_id, ke.model_id) IN (
                    SELECT file_id, model_id FROM knowledge_embedding_coverage_replace
                    WHERE chunk_rowid = NEW.chunk_rowid
                  ) OR (kcm.file_id = (SELECT file_id FROM knowledge_chunk_meta
                                       WHERE chunk_rowid = NEW.chunk_rowid)
                       AND ke.model_id = NEW.model_id))
            GROUP BY kcm.file_id, ke.model_id;
            DELETE FROM knowledge_embedding_coverage_replace
            WHERE chunk_rowid = NEW.chunk_rowid;
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
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + 1
            WHERE file_id = (SELECT file_id FROM knowledge_chunk_meta
                             WHERE chunk_rowid = NEW.chunk_rowid)
              AND model_id = NEW.model_id;
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT kcm.file_id, NEW.model_id, 1 FROM knowledge_chunk_meta kcm
            WHERE kcm.chunk_rowid = NEW.chunk_rowid
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage c
                              WHERE c.file_id = kcm.file_id AND c.model_id = NEW.model_id);
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_coverage_delete
         AFTER DELETE ON knowledge_embeddings BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - 1
            WHERE file_id = (SELECT file_id FROM knowledge_chunk_meta
                             WHERE chunk_rowid = OLD.chunk_rowid)
              AND model_id = OLD.model_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
            DELETE FROM knowledge_embedding_coverage_replace
            WHERE chunk_rowid = OLD.chunk_rowid;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_coverage_insert
         AFTER INSERT ON knowledge_chunk_meta
         BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = NEW.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = NEW.file_id
              AND NOT EXISTS (SELECT 1 FROM knowledge_chunk_meta_coverage_replace
                              WHERE chunk_rowid = NEW.chunk_rowid)
              AND EXISTS (SELECT 1 FROM knowledge_embeddings ke
                          WHERE ke.chunk_rowid = NEW.chunk_rowid
                            AND ke.model_id = knowledge_embedding_coverage.model_id);
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT NEW.file_id, ke.model_id, COUNT(*) FROM knowledge_embeddings ke
            WHERE ke.chunk_rowid = NEW.chunk_rowid
              AND NOT EXISTS (SELECT 1 FROM knowledge_chunk_meta_coverage_replace
                              WHERE chunk_rowid = NEW.chunk_rowid)
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage c
                              WHERE c.file_id = NEW.file_id AND c.model_id = ke.model_id)
            GROUP BY ke.model_id;
            DELETE FROM knowledge_embedding_coverage
            WHERE EXISTS (SELECT 1 FROM knowledge_chunk_meta_coverage_replace
                          WHERE chunk_rowid = NEW.chunk_rowid)
              AND file_id IN (
                    SELECT file_id FROM knowledge_chunk_meta_coverage_replace
                    WHERE chunk_rowid = NEW.chunk_rowid
                    UNION
                    SELECT NEW.file_id
                  );
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT kcm.file_id, ke.model_id, COUNT(*)
            FROM knowledge_chunk_meta kcm
            JOIN knowledge_embeddings ke ON ke.chunk_rowid = kcm.chunk_rowid
            WHERE EXISTS (SELECT 1 FROM knowledge_chunk_meta_coverage_replace
                          WHERE chunk_rowid = NEW.chunk_rowid)
              AND kcm.file_id IN (
                    SELECT file_id FROM knowledge_chunk_meta_coverage_replace
                    WHERE chunk_rowid = NEW.chunk_rowid
                    UNION
                    SELECT NEW.file_id
                  )
            GROUP BY kcm.file_id, ke.model_id;
            DELETE FROM knowledge_chunk_meta_coverage_replace
            WHERE chunk_rowid = NEW.chunk_rowid;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_chunk_meta_coverage_delete
         AFTER DELETE ON knowledge_chunk_meta BEGIN
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count - (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = OLD.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = OLD.file_id AND embedded_count > 0;
            DELETE FROM knowledge_embedding_coverage WHERE embedded_count = 0;
            DELETE FROM knowledge_chunk_meta_coverage_replace
            WHERE chunk_rowid = OLD.chunk_rowid;
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
            UPDATE knowledge_embedding_coverage
            SET embedded_count = embedded_count + (
                SELECT COUNT(*) FROM knowledge_embeddings ke
                WHERE ke.chunk_rowid = NEW.chunk_rowid
                  AND ke.model_id = knowledge_embedding_coverage.model_id
            )
            WHERE file_id = NEW.file_id
              AND EXISTS (SELECT 1 FROM knowledge_embeddings ke
                          WHERE ke.chunk_rowid = NEW.chunk_rowid
                            AND ke.model_id = knowledge_embedding_coverage.model_id);
            INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count)
            SELECT NEW.file_id, ke.model_id, COUNT(*)
            FROM knowledge_embeddings ke
            WHERE ke.chunk_rowid = NEW.chunk_rowid
              AND NOT EXISTS (SELECT 1 FROM knowledge_embedding_coverage c
                              WHERE c.file_id = NEW.file_id AND c.model_id = ke.model_id)
            GROUP BY ke.model_id;
         END;",
    )?;
    Ok(())
}
