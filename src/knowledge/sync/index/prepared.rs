use super::super::files::PreparedFile;
use super::{replace_file_evidence, FileEvidence};
use crate::embed::embedding_to_bytes;
use crate::knowledge::identifiers::write_chunk_identifiers;
use anyhow::Result;
use rusqlite::params;

pub(in crate::knowledge::sync) fn index_prepared_file(
    conn: &rusqlite::Connection,
    source_id: i64,
    file_id: i64,
    file: &PreparedFile,
    model_id: Option<&str>,
) -> Result<usize> {
    if model_id.is_some() && file.vectors.len() != file.chunks.len() {
        anyhow::bail!("prepared embedding count does not match prepared chunk count");
    }
    for (chunk_index, chunk) in file.chunks.iter().enumerate() {
        conn.execute(
            "INSERT INTO knowledge_chunks(title, content) VALUES (?1, ?2)",
            params![chunk.title, chunk.content],
        )?;
        let rowid = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO knowledge_chunks_trigram(rowid, title, content) VALUES (?1, ?2, ?3)",
            params![rowid, chunk.title, chunk.content],
        )?;

        conn.execute(
            "INSERT INTO knowledge_chunk_meta
                (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rowid,
                file_id,
                source_id,
                chunk.line_start,
                chunk.line_end,
                file.metadata
            ],
        )?;

        write_chunk_identifiers(conn, rowid, &format!("{} {}", chunk.title, chunk.content))?;
        if let Some(model_id) = model_id {
            let embedding = &file.vectors[chunk_index];
            let blob = embedding_to_bytes(embedding);
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![rowid, blob, embedding.len() as i64, model_id],
            )?;
        }
    }

    conn.execute(
        "UPDATE knowledge_files SET content_hash = ?1, size_bytes = ?2,
            chunk_count = ?3, chunker_version = ?4, indexed_at = datetime('now'),
            mtime_ns = ?5, stat_checked_ns = ?6
         WHERE id = ?7",
        params![
            file.hash,
            file.size_bytes,
            file.chunks.len() as i64,
            file.chunker_version,
            file.stat.mtime_ns,
            file.stat.checked_ns,
            file_id
        ],
    )?;
    replace_file_evidence(
        conn,
        FileEvidence {
            source_id,
            file_id,
            rel_path: &file.rel_path,
            status: file.symbol_status,
            symbols: &file.symbols,
            references: &file.references,
            entries: &file.entries,
        },
    )?;

    Ok(file.chunks.len())
}
