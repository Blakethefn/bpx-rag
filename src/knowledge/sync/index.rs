mod edges;
mod prepared;
mod wikilink;

pub(in crate::knowledge::sync) use prepared::index_prepared_file;

pub(in crate::knowledge::sync) use edges::resolve_code_edges;
#[cfg(test)]
pub(in crate::knowledge::sync) use wikilink::resolve_wikilink;
pub(in crate::knowledge::sync) use wikilink::resolve_wikilinks_for_source;

use super::files::PreparedFile;
use crate::chunk::evidence::{EntryObservation, Reference};
use crate::chunk::Symbol;
use crate::embed::normalize_embedding;
use crate::knowledge::entries::replace_file_entries;
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

pub(super) struct FileEvidence<'a> {
    pub(super) source_id: i64,
    pub(super) file_id: i64,
    pub(super) rel_path: &'a str,
    pub(super) status: &'a str,
    pub(super) symbols: &'a [Symbol],
    pub(super) references: &'a [Reference],
    pub(super) entries: &'a [EntryObservation],
}

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

pub(super) fn delete_file_chunks(conn: &rusqlite::Connection, file_id: i64) -> Result<()> {
    let rowids: Vec<i64> = {
        let mut stmt =
            conn.prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

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
