use anyhow::{anyhow, Result};
use rusqlite::OptionalExtension;

use super::remove_failpoint;
use super::scope;

#[derive(Debug, Clone)]
pub struct KnowledgeSourceInfo {
    pub id: i64,
    pub label: String,
    pub path: String,
    pub patterns: Vec<String>,
    pub enrichments: Vec<String>,
    pub created_at: String,
    pub last_sync: Option<String>,
    pub file_count: i64,
    pub chunk_count: i64,
}

#[derive(Debug)]
pub struct RemoveResult {
    pub files_removed: i64,
    pub chunks_removed: i64,
}

fn parse_enrichments(json: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn row_to_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeSourceInfo> {
    let enrichments_json: String = row.get(4)?;
    Ok(KnowledgeSourceInfo {
        id: row.get(0)?,
        label: row.get(1)?,
        path: row.get(2)?,
        patterns: scope::decode(row.get::<_, Option<String>>(3)?.as_deref()),
        enrichments: parse_enrichments(&enrichments_json)?,
        created_at: row.get(5)?,
        last_sync: row.get(6)?,
        file_count: row.get(7)?,
        chunk_count: row.get(8)?,
    })
}

const SOURCE_COLUMNS: &str =
    "id, label, path, glob, enrichments, created_at, last_sync, file_count, chunk_count";

impl super::KnowledgeStore {
    pub fn add_source(
        &self,
        label: &str,
        path: &str,
        patterns: &[String],
        enrichments: &[String],
    ) -> Result<i64> {
        let enrichments_json = serde_json::to_string(enrichments)?;
        let stored_scope = scope::encode(patterns);
        self.conn.execute(
            "INSERT INTO knowledge_sources (label, path, glob, enrichments)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![label, path, stored_scope, enrichments_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_source(&self, label: &str) -> Result<RemoveResult> {
        let source = self
            .get_source(label)?
            .ok_or_else(|| anyhow!("Knowledge source '{}' not found", label))?;

        let source_id = source.id;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let mut step = 0usize;

        let chunk_rowids: Vec<i64> = {
            let mut stmt = transaction
                .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE source_id = ?1")?;
            let rows = stmt.query_map(rusqlite::params![source_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let chunks_removed = chunk_rowids.len() as i64;

        let files_removed: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1",
            rusqlite::params![source_id],
            |row| row.get(0),
        )?;

        for rowid in &chunk_rowids {
            transaction.execute(
                "DELETE FROM knowledge_chunks WHERE rowid = ?1",
                rusqlite::params![rowid],
            )?;
            transaction.execute(
                "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
                rusqlite::params![rowid],
            )?;
            transaction.execute(
                "DELETE FROM knowledge_identifiers WHERE rowid = ?1",
                rusqlite::params![rowid],
            )?;
            transaction.execute(
                "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
                rusqlite::params![rowid],
            )?;
            remove_failpoint(&mut step)?;
        }

        transaction.execute(
            "DELETE FROM knowledge_resolutions WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        transaction.execute(
            "DELETE FROM knowledge_findings WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        transaction.execute(
            "DELETE FROM knowledge_analysis WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        transaction.execute(
            "DELETE FROM knowledge_references WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        transaction.execute(
            "DELETE FROM knowledge_entries WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        transaction.execute(
            "DELETE FROM knowledge_symbols WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        remove_failpoint(&mut step)?;

        transaction.execute(
            "DELETE FROM knowledge_chunk_meta WHERE source_id = ?1",
            rusqlite::params![source_id],
        )?;
        remove_failpoint(&mut step)?;

        transaction.execute(
            "DELETE FROM knowledge_sources WHERE id = ?1",
            rusqlite::params![source_id],
        )?;
        remove_failpoint(&mut step)?;

        transaction.commit()?;
        Ok(RemoveResult {
            files_removed,
            chunks_removed,
        })
    }

    pub fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SOURCE_COLUMNS} FROM knowledge_sources ORDER BY label"
        ))?;
        let sources: Vec<KnowledgeSourceInfo> = stmt
            .query_map([], row_to_source)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(sources)
    }

    pub fn get_source(&self, label: &str) -> Result<Option<KnowledgeSourceInfo>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SOURCE_COLUMNS} FROM knowledge_sources WHERE label = ?1"
        ))?;
        let result = stmt
            .query_row(rusqlite::params![label], row_to_source)
            .optional()?;
        Ok(result)
    }

    pub fn set_watch(&self, label: &str, enabled: bool) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE knowledge_sources SET watch_enabled = ?1 WHERE label = ?2",
            rusqlite::params![enabled, label],
        )?;
        if updated == 0 {
            return Err(anyhow!("knowledge source '{}' was not found", label));
        }
        Ok(())
    }

    pub fn watched_sources(&self) -> Result<Vec<KnowledgeSourceInfo>> {
        let mut statement = self.conn.prepare(
            "SELECT label FROM knowledge_sources WHERE watch_enabled = 1 ORDER BY label",
        )?;
        let labels = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut sources = Vec::with_capacity(labels.len());
        for label in labels {
            if let Some(source) = self.get_source(&label)? {
                sources.push(source);
            }
        }
        Ok(sources)
    }
}
