use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(in crate::knowledge::schema) fn ensure_database_instance_id(conn: &Connection) -> Result<()> {
    let id: String = conn.query_row(
        "SELECT database_id FROM knowledge_embedding_state WHERE state_id = 1",
        [],
        |row| row.get(0),
    )?;
    if id.is_empty() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seed = format!(
            "{nanos}:{}:{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let instance_id = format!("{:x}", Sha256::digest(seed.as_bytes()));
        conn.execute(
            "UPDATE knowledge_embedding_state SET database_id = ?1 WHERE state_id = 1",
            [&instance_id],
        )?;
    }
    Ok(())
}
