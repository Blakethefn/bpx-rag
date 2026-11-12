use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection};

use super::KnowledgeStore;

pub(crate) const SYMBOL_INDEX_VERSION: i64 = 2;

const MAX_NAME_BYTES: usize = 256;

const PREFIX_UPPER_BOUND: char = '\u{10FFFF}';

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

mod write;
pub(crate) use write::{delete_file_symbols, replace_file_symbols};

#[cfg(test)]
mod tests;
