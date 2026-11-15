use super::super::in_memory_database_bytes;
use super::ScalingEmbedder;
use crate::embed::{self};
use crate::knowledge;
use crate::knowledge::KnowledgeStore;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Instant;

pub(super) struct SyntheticIndex {
    pub(super) chunk_count: usize,
    pub(super) store: KnowledgeStore,
    pub(super) indexing_ms: f64,
    pub(super) database_bytes: u64,
}

pub(super) fn index_synthetic(chunk_count: usize) -> Result<SyntheticIndex> {
    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(ScalingEmbedder));
    store.add_source("scaling", "/synthetic/bpx-rag-scaling", &[], &[])?;
    let source = store
        .get_source("scaling")?
        .ok_or_else(|| anyhow!("could not load synthetic scaling source"))?;

    let index_start = Instant::now();
    let conn = store.conn();
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let index_result = (|| -> Result<()> {
        for index in 0..chunk_count {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                 VALUES (?1, ?2, 'synthetic', 4096)",
                rusqlite::params![source.id, format!("src/chunk-{index}.rs")],
            )?;
            let file_id = conn.last_insert_rowid();
            let title = format!("synthetic item {index}");
            let content = format!(
                "Benchmark-only unrelated code note with stable filler terms for chunk {index}."
            );
            conn.execute(
                "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
                rusqlite::params![title, content],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
                rusqlite::params![rowid, title, content],
            )?;
            conn.execute(
                "INSERT INTO knowledge_chunk_meta
                 (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, '{}')",
                rusqlite::params![rowid, file_id, source.id, 1, 4],
            )?;
            let vector = ScalingEmbedder::vector_for(index);
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    rowid,
                    embed::embedding_to_bytes(&vector),
                    ScalingEmbedder::DIMENSION as i64,
                    ScalingEmbedder::MODEL_ID
                ],
            )?;
        }
        Ok(())
    })();
    match index_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    let indexing_ms = index_start.elapsed().as_secs_f64() * 1000.0;
    let database_bytes = in_memory_database_bytes(conn)?;
    Ok(SyntheticIndex {
        chunk_count,
        store,
        indexing_ms,
        database_bytes,
    })
}
