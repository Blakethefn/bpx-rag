mod edges;
mod wikilink;

pub(in crate::knowledge::sync) use edges::resolve_code_edges;
#[cfg(test)]
pub(in crate::knowledge::sync) use wikilink::resolve_wikilink;
pub(in crate::knowledge::sync) use wikilink::resolve_wikilinks_for_source;

use super::files::PreparedFile;
use crate::chunk::evidence::{EntryObservation, Reference};
use crate::chunk::Symbol;
use crate::embed::{embedding_to_bytes, normalize_embedding};
use crate::knowledge::entries::replace_file_entries;
use crate::knowledge::identifiers::write_chunk_identifiers;
use crate::knowledge::references::{delete_file_references, replace_file_references};
use crate::knowledge::symbols::{delete_file_symbols, replace_file_symbols};
use anyhow::{Context, Result};
use rusqlite::params;

pub(super) fn embed_pending_batch(
    embedder: &dyn crate::embed::Embed,
    prepared_files: &mut [PreparedFile],
    pending: &mut Vec<(usize, usize, String)>,
) -> Result<()> {
    let texts = pending
        .iter()
        .map(|(_, _, text)| text.as_str())
        .collect::<Vec<_>>();
    let embeddings = embedder
        .embed_batch(&texts)
        .context("failed to embed changed corpus chunks")?;
    if embeddings.len() != pending.len() {
        anyhow::bail!(
            "embedder returned {} vectors for {} chunks",
            embeddings.len(),
            pending.len()
        );
    }
    for ((file_index, chunk_index, _), vector) in pending.drain(..).zip(embeddings) {
        prepared_files[file_index].vectors[chunk_index] =
            normalize_embedding(&vector, embedder.dim())?;
    }
    Ok(())
}

/// Insert pre-chunked content and precomputed vectors within the caller's
/// transaction. All expensive enrichment and model inference happened earlier.
pub(super) fn index_prepared_file(
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
        // FTS5 (BM25)
        conn.execute(
            "INSERT INTO knowledge_chunks(title, content) VALUES (?1, ?2)",
            params![chunk.title, chunk.content],
        )?;
        let rowid = conn.last_insert_rowid();

        // Trigram FTS5 with explicit rowid
        conn.execute(
            "INSERT INTO knowledge_chunks_trigram(rowid, title, content) VALUES (?1, ?2, ?3)",
            params![rowid, chunk.title, chunk.content],
        )?;

        // Chunk metadata. Its insert trigger queues the chunk for identifier indexing.
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

        // Identifier words, keyed by the same rowid. This replaces any row left under the
        // rowid and clears the queue entry the insert above created.
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

    // Update file record first: the symbol rows are stamped with the content hash it stores.
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

/// The evidence rows for one file: symbols, references, and entries, with the status that
/// labels the symbol rows.
pub(super) struct FileEvidence<'a> {
    pub(super) source_id: i64,
    pub(super) file_id: i64,
    pub(super) rel_path: &'a str,
    pub(super) status: &'a str,
    pub(super) symbols: &'a [Symbol],
    pub(super) references: &'a [Reference],
    pub(super) entries: &'a [EntryObservation],
}

/// Write symbols, references, and entries for one file, then stamp both hashes from the
/// content hash already stored on the file row.
pub(super) fn replace_file_evidence(
    conn: &rusqlite::Connection,
    evidence: FileEvidence<'_>,
) -> Result<()> {
    let FileEvidence {
        source_id,
        file_id,
        rel_path,
        status,
        symbols,
        references,
        entries,
    } = evidence;
    let evidence_hash: String = conn.query_row(
        "SELECT content_hash FROM knowledge_files WHERE id = ?1",
        params![file_id],
        |row| row.get(0),
    )?;
    replace_file_symbols(conn, source_id, file_id, status, symbols)?;
    replace_file_references(conn, source_id, file_id, status, references)?;
    replace_file_entries(conn, source_id, file_id, rel_path, &evidence_hash, entries)?;
    Ok(())
}

/// Delete all chunks (FTS5, trigram, embeddings, metadata) for a given file.
pub(super) fn delete_file_chunks(conn: &rusqlite::Connection, file_id: i64) -> Result<()> {
    let rowids: Vec<i64> = {
        let mut stmt =
            conn.prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    // FTS5 tables: delete one at a time (safest for all SQLite versions)
    for rowid in &rowids {
        conn.execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![rowid],
        )?;
        conn.execute(
            "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
            params![rowid],
        )?;
        conn.execute(
            "DELETE FROM knowledge_identifiers WHERE rowid = ?1",
            params![rowid],
        )?;
    }

    // Embeddings and metadata (regular tables, bulk delete is fine)
    conn.execute(
        "DELETE FROM knowledge_embeddings WHERE chunk_rowid IN
         (SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1)",
        params![file_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk_meta WHERE file_id = ?1",
        params![file_id],
    )?;
    delete_file_symbols(conn, file_id)?;
    delete_file_references(conn, file_id)?;

    Ok(())
}
