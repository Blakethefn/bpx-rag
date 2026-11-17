use anyhow::Result;
use rusqlite::{params, Connection};

use crate::knowledge::filter::file_extension;

pub(in crate::knowledge::schema) fn ensure_file_extension(conn: &Connection) -> Result<()> {
    if has_extension_column(conn)? && !has_unindexed_paths(conn)? {
        return Ok(());
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        if !has_extension_column(conn)? {
            conn.execute(
                "ALTER TABLE knowledge_files ADD COLUMN extension TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }

        let files = conn
            .prepare(
                "SELECT id, rel_path FROM knowledge_files
                 WHERE extension = '' AND instr(rel_path, '.') > 0",
            )?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (file_id, rel_path) in files {
            let extension = file_extension(&rel_path);
            if !extension.is_empty() {
                conn.execute(
                    "UPDATE knowledge_files SET extension = ?1 WHERE id = ?2",
                    params![extension, file_id],
                )?;
            }
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_files_extension ON knowledge_files(extension)",
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
    Ok(())
}

fn has_extension_column(conn: &Connection) -> Result<bool> {
    let mut statement = conn.prepare("PRAGMA table_info(knowledge_files)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns.iter().any(|column| column == "extension"))
}

fn has_unindexed_paths(conn: &Connection) -> Result<bool> {
    let paths = conn
        .prepare("SELECT rel_path FROM knowledge_files WHERE extension = ''")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths.iter().any(|path| !file_extension(path).is_empty()))
}
