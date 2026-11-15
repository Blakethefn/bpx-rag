use anyhow::{anyhow, Result};

use super::super::remove_failpoint;
use super::RemoveResult;

impl super::super::KnowledgeStore {
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
        if let Some(index_dir) = self.vector_index_dir.as_deref() {
            if let Err(error) = super::super::ann::remove_source_sidecars(index_dir, source_id) {
                eprintln!("[bpx-rag] could not clear ANN sidecars for source {label}: {error}");
            }
        }
        Ok(RemoveResult {
            files_removed,
            chunks_removed,
        })
    }
}
