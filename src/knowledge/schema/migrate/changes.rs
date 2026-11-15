use anyhow::Result;
use rusqlite::Connection;

const TRIGGERS: [&str; 3] = [
    "knowledge_embeddings_generation_insert",
    "knowledge_embeddings_generation_update",
    "knowledge_embeddings_generation_delete",
];

pub(in crate::knowledge::schema) fn ensure_embedding_change_tracking(
    conn: &Connection,
) -> Result<()> {
    ensure_source_column(conn)?;
    replace_unscoped_triggers(conn)?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_embedding_changes_source
             ON knowledge_embedding_changes(source_id, change_id);
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_insert
         AFTER INSERT ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes
                (chunk_rowid, model_id, operation, source_id)
            VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert',
                (SELECT source_id FROM knowledge_chunk_meta
                 WHERE chunk_rowid = NEW.chunk_rowid));
            UPDATE knowledge_embedding_state
            SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_update
         AFTER UPDATE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes
                (chunk_rowid, model_id, operation, source_id)
            VALUES (NEW.chunk_rowid, NEW.model_id, 'upsert',
                (SELECT source_id FROM knowledge_chunk_meta
                 WHERE chunk_rowid = NEW.chunk_rowid));
            UPDATE knowledge_embedding_state
            SET generation = last_insert_rowid() WHERE state_id = 1;
         END;
         CREATE TRIGGER IF NOT EXISTS knowledge_embeddings_generation_delete
         AFTER DELETE ON knowledge_embeddings BEGIN
            INSERT INTO knowledge_embedding_changes
                (chunk_rowid, model_id, operation, source_id)
            VALUES (OLD.chunk_rowid, OLD.model_id, 'delete',
                (SELECT source_id FROM knowledge_chunk_meta
                 WHERE chunk_rowid = OLD.chunk_rowid));
            UPDATE knowledge_embedding_state
            SET generation = last_insert_rowid() WHERE state_id = 1;
         END;",
    )?;
    Ok(())
}

fn ensure_source_column(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare("PRAGMA table_info(knowledge_embedding_changes)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|column| column == "source_id") {
        conn.execute(
            "ALTER TABLE knowledge_embedding_changes ADD COLUMN source_id INTEGER",
            [],
        )?;
    }
    Ok(())
}

fn replace_unscoped_triggers(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'trigger' AND name IN (?1, ?2, ?3)
           AND instr(sql, 'source_id') = 0",
    )?;
    let old = statement
        .query_map(TRIGGERS, |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in old {
        conn.execute_batch(&format!("DROP TRIGGER {name}"))?;
    }
    Ok(())
}
