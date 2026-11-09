use super::*;
use crate::embed::{embedding_to_bytes, normalize_embedding};
use crate::knowledge::entries::replace_file_entries;
use crate::knowledge::identifiers::write_chunk_identifiers;
use crate::knowledge::references::{delete_file_references, replace_file_references};
use crate::knowledge::symbols::{delete_file_symbols, replace_file_symbols};
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::HashMap;

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
        source_id,
        file_id,
        &file.rel_path,
        file.symbol_status,
        &file.symbols,
        &file.references,
        &file.entries,
    )?;

    Ok(file.chunks.len())
}

/// Write symbols, references, and entries for one file, then stamp both hashes from the
/// content hash already stored on the file row.
pub(super) fn replace_file_evidence(
    conn: &rusqlite::Connection,
    source_id: i64,
    file_id: i64,
    rel_path: &str,
    status: &str,
    symbols: &[Symbol],
    references: &[Reference],
    entries: &[EntryObservation],
) -> Result<()> {
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

/// Resolve an Obsidian wikilink target to a file ID.
///
/// Resolution order:
/// 1. Exact match on rel_path
/// 2. Exact match with `.md` appended
/// 3. Basename match (target matches the filename portion of any rel_path)
pub(super) fn resolve_wikilink(target: &str, files: &HashMap<String, i64>) -> Option<i64> {
    // Exact match
    if let Some(&id) = files.get(target) {
        return Some(id);
    }
    // With .md extension
    let with_md = format!("{}.md", target);
    if let Some(&id) = files.get(&with_md) {
        return Some(id);
    }
    // Basename match: target is just a note name without path
    let suffix_md = format!("/{}.md", target);
    let suffix_plain = format!("/{}", target);
    for (path, &id) in files {
        if path.ends_with(&suffix_md) || path.ends_with(&suffix_plain) {
            return Some(id);
        }
    }
    None
}

/// Wikilink targets stored in a chunk's metadata. A missing key means no links.
fn wikilink_targets(meta: &serde_json::Value) -> Vec<String> {
    meta.get("wikilinks")
        .and_then(serde_json::Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Delete all wikilink graph edges for a source, then re-resolve from stored
/// metadata. Runs inside the sync transaction after all files are indexed.
///
/// Returns the number of links created.
pub(super) fn resolve_wikilinks_for_source(
    conn: &rusqlite::Connection,
    source_id: i64,
) -> Result<usize> {
    // Delete existing wikilink edges for this source's files. Code edges are
    // owned by `resolve_code_edges`.
    conn.execute(
        "DELETE FROM knowledge_links WHERE link_type = 'wikilink' AND source_file_id IN
         (SELECT id FROM knowledge_files WHERE source_id = ?1)",
        params![source_id],
    )?;

    // Load all file rel_paths → IDs for this source
    let files: HashMap<String, i64> = {
        let mut stmt =
            conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(0)?))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()?
    };

    if files.is_empty() {
        return Ok(0);
    }

    // For each file, get one metadata row and extract wikilinks
    let file_wikilinks: Vec<(i64, Vec<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT kf.id, kcm.metadata
             FROM knowledge_files kf
             JOIN knowledge_chunk_meta kcm ON kcm.file_id = kf.id
             WHERE kf.source_id = ?1
             GROUP BY kf.id",
        )?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut file_wikilinks = Vec::new();
        for (file_id, meta_json) in rows.collect::<rusqlite::Result<Vec<_>>>()? {
            let meta: serde_json::Value = serde_json::from_str(&meta_json)
                .with_context(|| format!("invalid chunk metadata for file {file_id}"))?;
            let targets = wikilink_targets(&meta);
            if !targets.is_empty() {
                file_wikilinks.push((file_id, targets));
            }
        }
        file_wikilinks
    };

    // Resolve and insert links
    let mut links_created = 0usize;
    let mut insert_stmt = conn.prepare(
        "INSERT OR IGNORE INTO knowledge_links
            (source_file_id, target_file_id, link_type, confidence)
         VALUES (?1, ?2, ?3, ?4)",
    )?;

    for (source_file_id, targets) in &file_wikilinks {
        for target in targets {
            if let Some(target_file_id) = resolve_wikilink(target, &files) {
                if target_file_id != *source_file_id {
                    insert_stmt.execute(params![
                        source_file_id,
                        target_file_id,
                        "wikilink",
                        1.0
                    ])?;
                    links_created += 1;
                }
            }
        }
    }

    Ok(links_created)
}

// ── Tests ───────────────────────────────────────────────────────────────────

/// Replace this source's code edges (imports, includes, and module references)
/// with the relations among its indexed files. Edges are stored as
/// `code:<kind>` so wikilink edges are left alone.
///
/// Returns the number of edges created.
pub(super) fn resolve_code_edges(
    conn: &rusqlite::Connection,
    source_id: i64,
    root: &std::path::Path,
) -> Result<usize> {
    conn.execute(
        "DELETE FROM knowledge_links WHERE link_type LIKE 'code:%' AND source_file_id IN
         (SELECT id FROM knowledge_files WHERE source_id = ?1)",
        params![source_id],
    )?;

    let indexed: Vec<(i64, String)> = {
        let mut stmt =
            conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
        let rows = stmt.query_map(params![source_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let id_by_path: HashMap<&str, i64> = indexed
        .iter()
        .map(|(id, rel_path)| (rel_path.as_str(), *id))
        .collect();
    let abs_paths: Vec<std::path::PathBuf> = indexed
        .iter()
        .map(|(_, rel_path)| root.join(rel_path))
        .collect();

    let edges = crate::relations::project_edges(root, &abs_paths)?;
    let mut insert = conn.prepare(
        "INSERT OR IGNORE INTO knowledge_links
            (source_file_id, target_file_id, link_type, confidence)
         VALUES (?1, ?2, ?3, 1.0)",
    )?;
    let mut created = 0usize;
    for edge in &edges {
        // Markdown wikilinks are the wikilink enrichment's job; code relations only.
        if edge.kind == "wikilink" {
            continue;
        }
        let (Some(&from), Some(&to)) = (
            id_by_path.get(edge.from.as_str()),
            id_by_path.get(edge.to.as_str()),
        ) else {
            continue;
        };
        created += insert.execute(params![from, to, format!("code:{}", edge.kind)])?;
    }
    Ok(created)
}
