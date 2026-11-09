//! Symbol index: rows written by sync and exact or prefix lookup over them.
//!
//! Rows are written in the same transaction as a file's chunks. A file has current rows only
//! when its `symbol_version` is at least `SYMBOL_INDEX_VERSION` and its `symbols_hash` equals its
//! `content_hash`. Any other file is pending, and sync backfills it. Lookup returns no rows
//! for a pending file, so a stale line range is never cited.

use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection};

use super::KnowledgeStore;
use crate::chunk::Symbol;

/// Version of the symbol index. Bump it when extraction changes what it writes. A bump
/// backfills symbols on the next sync without re-chunking or re-embedding.
pub(crate) const SYMBOL_INDEX_VERSION: i64 = 1;

/// Longest accepted name. Identifiers are far shorter, so a longer query is an error.
const MAX_NAME_BYTES: usize = 256;

/// The largest code point. A prefix range ends just before `prefix + U+10FFFF`.
const PREFIX_UPPER_BOUND: char = '\u{10FFFF}';

/// How `name` is matched against stored symbol names. Both modes are case-sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Exact,
    Prefix,
}

#[derive(Debug, Clone)]
pub struct SymbolQuery<'a> {
    pub name: &'a str,
    pub mode: MatchMode,
    pub source: Option<&'a str>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolHit {
    pub source_label: String,
    pub rel_path: String,
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
}

/// Files in a source that have not yet been symbol-indexed. Sync backfills them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSymbols {
    pub source_label: String,
    pub files: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolReport {
    pub hits: Vec<SymbolHit>,
    pub truncated: bool,
    pub pending: Vec<PendingSymbols>,
}

const HIT_COLUMNS: &str =
    "s.label, kf.rel_path, ks.name, ks.kind, ks.container, ks.line_start, ks.line_end";

/// Look up declarations by name. Errors on an empty or oversized name, or an unknown source.
/// Duplicate names are all returned, ordered by name, source, path, and line.
pub fn lookup(store: &KnowledgeStore, query: &SymbolQuery<'_>) -> Result<SymbolReport> {
    let name = query.name.trim();
    if name.is_empty() {
        bail!("symbol name is required");
    }
    if name.len() > MAX_NAME_BYTES {
        bail!("symbol name must be at most {MAX_NAME_BYTES} bytes");
    }
    let source_id = match query.source {
        Some(label) => Some(
            store
                .get_source(label)?
                .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"))?
                .id,
        ),
        None => None,
    };

    let conn = store.conn();
    let hits = select_hits(conn, name, query.mode, source_id, query.limit)?;
    let truncated = hits.len() > query.limit;
    let hits = hits.into_iter().take(query.limit).collect();
    let pending = pending_sources(conn, source_id)?;
    Ok(SymbolReport {
        hits,
        truncated,
        pending,
    })
}

fn select_hits(
    conn: &Connection,
    name: &str,
    mode: MatchMode,
    source_id: Option<i64>,
    limit: usize,
) -> Result<Vec<SymbolHit>> {
    let predicate = match mode {
        MatchMode::Exact => "ks.name = ?1",
        MatchMode::Prefix => "ks.name >= ?1 AND ks.name < ?4",
    };
    let sql = format!(
        "SELECT {HIT_COLUMNS}
         FROM knowledge_symbols ks
         JOIN knowledge_sources s ON s.id = ks.source_id
         JOIN knowledge_files kf ON kf.id = ks.file_id
         WHERE {predicate} AND (?2 IS NULL OR ks.source_id = ?2)
           AND kf.symbol_version >= {SYMBOL_INDEX_VERSION}
           AND kf.symbols_hash IS kf.content_hash
         ORDER BY ks.name, s.label, kf.rel_path, ks.line_start
         LIMIT ?3"
    );
    // One extra row tells the caller whether the result was truncated.
    let fetch_limit = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let upper_bound = format!("{name}{PREFIX_UPPER_BOUND}");
    let mut statement = conn.prepare(&sql)?;
    let rows = match mode {
        MatchMode::Exact => {
            statement.query_map(params![name, source_id, fetch_limit], hit_from_row)?
        }
        MatchMode::Prefix => statement.query_map(
            params![name, source_id, fetch_limit, upper_bound],
            hit_from_row,
        )?,
    };
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn hit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolHit> {
    Ok(SymbolHit {
        source_label: row.get(0)?,
        rel_path: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        container: row.get(4)?,
        line_start: row.get(5)?,
        line_end: row.get(6)?,
    })
}

fn pending_sources(conn: &Connection, source_id: Option<i64>) -> Result<Vec<PendingSymbols>> {
    let mut statement = conn.prepare(
        "SELECT s.label, COUNT(*)
         FROM knowledge_files kf
         JOIN knowledge_sources s ON s.id = kf.source_id
         WHERE (kf.symbol_version < ?1 OR kf.symbols_hash IS NOT kf.content_hash)
           AND (?2 IS NULL OR kf.source_id = ?2)
         GROUP BY s.label
         ORDER BY s.label",
    )?;
    let rows = statement.query_map(params![SYMBOL_INDEX_VERSION, source_id], |row| {
        Ok(PendingSymbols {
            source_label: row.get(0)?,
            files: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Replace a file's symbol rows and mark the file as current. Runs inside the sync transaction.
///
/// The file's `content_hash` must already hold the content the symbols came from. The rows
/// are stamped with that hash, so a later content change by an older writer marks them stale.
/// `status` is written in the same update. Only `complete` stores symbol rows: an empty list
/// then means the walk finished and the file declared nothing.
pub(crate) fn replace_file_symbols(
    conn: &Connection,
    source_id: i64,
    file_id: i64,
    status: &str,
    symbols: &[Symbol],
) -> Result<()> {
    delete_file_symbols(conn, file_id)?;
    if status == crate::chunk::evidence::STATUS_COMPLETE {
        let mut insert = conn.prepare_cached(
            "INSERT INTO knowledge_symbols
                (source_id, file_id, name, kind, container, line_start, line_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for symbol in symbols {
            insert.execute(params![
                source_id,
                file_id,
                symbol.name,
                symbol.kind,
                symbol.container,
                symbol.line_start,
                symbol.line_end
            ])?;
        }
    }
    conn.execute(
        "UPDATE knowledge_files
         SET symbol_version = ?1,
             symbol_status = ?2,
             symbols_hash = (SELECT content_hash FROM knowledge_files WHERE id = ?3)
         WHERE id = ?3",
        params![SYMBOL_INDEX_VERSION, status, file_id],
    )?;
    Ok(())
}

/// Remove one file's symbol rows. Called wherever the file's chunks are removed.
pub(crate) fn delete_file_symbols(conn: &Connection, file_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM knowledge_symbols WHERE file_id = ?1",
        params![file_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
