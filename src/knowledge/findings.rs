//! Derived conclusions for one source.
//!
//! `knowledge_analysis` is one row per source. `knowledge_findings` holds only conclusions, and
//! only when the analysis is complete. A reader returns a finding when its version, generation,
//! and fingerprint match that row.

use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};

use super::KnowledgeStore;
use crate::chunk::evidence::{STATUS_COMPLETE, STATUS_UNSUPPORTED};

pub(crate) const ANALYSIS_VERSION: i64 = 1;

const UNUSED_REASON: &str = "no indexed reference";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileRow {
    id: i64,
    rel_path: String,
    content_hash: String,
    symbol_status: String,
    reference_status: String,
    reference_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryRow {
    target_file_id: Option<i64>,
    target_rel_path: String,
    kind: String,
    evidence_hash: String,
    evidence_file_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolRow {
    id: i64,
    file_id: i64,
    name: String,
}

/// Rewrite resolutions and the analysis row from the evidence already stored for the source.
pub(crate) fn rebuild_source(conn: &Connection, source_id: i64) -> Result<()> {
    crate::knowledge::evidence::relink_entries(conn, source_id)?;
    crate::knowledge::resolution::rebuild(conn, source_id)?;
    rebuild_analysis(conn, source_id)
}

fn rebuild_analysis(conn: &Connection, source_id: i64) -> Result<()> {
    let files = load_files(conn, source_id)?;
    let entries = load_entries(conn, source_id)?;
    let symbols = load_symbols(conn, source_id)?;
    let previous: i64 = conn
        .query_row(
            "SELECT evidence_generation FROM knowledge_analysis WHERE source_id = ?1",
            params![source_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let generation = previous + 1;
    let fingerprint = fingerprint(&files, &entries);
    let gaps = gaps(&files, &entries);
    let status = if incomplete(&files) {
        "incomplete"
    } else {
        "complete"
    };

    conn.execute(
        "DELETE FROM knowledge_findings WHERE source_id = ?1",
        params![source_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_analysis WHERE source_id = ?1",
        params![source_id],
    )?;
    conn.execute(
        "INSERT INTO knowledge_analysis
            (source_id, analysis_version, evidence_generation, fingerprint, status, gaps)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            source_id,
            ANALYSIS_VERSION,
            generation,
            fingerprint,
            status,
            gaps
        ],
    )?;
    if status == "complete" {
        write_findings(
            conn,
            source_id,
            generation,
            &fingerprint,
            &files,
            &entries,
            &symbols,
        )?;
    }
    Ok(())
}

fn write_findings(
    conn: &Connection,
    source_id: i64,
    generation: i64,
    fingerprint: &str,
    files: &[FileRow],
    entries: &[EntryRow],
    symbols: &[SymbolRow],
) -> Result<()> {
    let used = used_symbols(conn, source_id)?;
    let (reached, test_reached, program_roots, test_roots) =
        reachability(conn, source_id, entries)?;
    let complete_files: HashSet<i64> = files
        .iter()
        .filter(|file| file.symbol_status == STATUS_COMPLETE)
        .map(|file| file.id)
        .collect();

    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_findings
            (source_id, kind, file_id, symbol_id, reason, analysis_version, evidence_generation, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    let mut unused: Vec<&SymbolRow> = symbols
        .iter()
        .filter(|symbol| complete_files.contains(&symbol.file_id) && !used.contains(&symbol.id))
        .collect();
    unused.sort_by(|left, right| {
        let left_path = files
            .iter()
            .find(|file| file.id == left.file_id)
            .map(|file| file.rel_path.as_str())
            .unwrap_or("");
        let right_path = files
            .iter()
            .find(|file| file.id == right.file_id)
            .map(|file| file.rel_path.as_str())
            .unwrap_or("");
        (left_path, left.name.as_str(), left.id).cmp(&(right_path, right.name.as_str(), right.id))
    });
    for symbol in unused {
        insert.execute(params![
            source_id,
            "possibly_unused_symbol",
            symbol.file_id,
            symbol.id,
            UNUSED_REASON,
            ANALYSIS_VERSION,
            generation,
            fingerprint
        ])?;
    }

    let mut file_ids: Vec<&FileRow> = files.iter().collect();
    file_ids.sort_by(|left, right| {
        left.rel_path
            .cmp(&right.rel_path)
            .then(left.id.cmp(&right.id))
    });
    let program_reason = root_reason(
        "not reached from program or library_root",
        &program_roots,
        files,
    );
    let test_reason = root_reason("reachable only from test entries", &test_roots, files);
    for file in file_ids {
        if reached.contains(&file.id) {
            continue;
        }
        let reason = if test_reached.contains(&file.id) {
            test_reason.clone()
        } else {
            program_reason.clone()
        };
        insert.execute(params![
            source_id,
            "unreachable_file",
            file.id,
            None::<i64>,
            reason,
            ANALYSIS_VERSION,
            generation,
            fingerprint
        ])?;
    }
    Ok(())
}

fn used_symbols(conn: &Connection, source_id: i64) -> Result<HashSet<i64>> {
    let mut statement = conn.prepare(
        "SELECT target_symbol_id FROM knowledge_resolutions
         WHERE source_id = ?1
           AND status IN ('resolved', 'ambiguous')
           AND target_symbol_id IS NOT NULL",
    )?;
    let rows = statement.query_map(params![source_id], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

fn reachability(
    conn: &Connection,
    source_id: i64,
    entries: &[EntryRow],
) -> Result<(HashSet<i64>, HashSet<i64>, Vec<i64>, Vec<i64>)> {
    let mut edges: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut statement = conn.prepare(
        "SELECT kr.file_id, res.target_file_id
         FROM knowledge_resolutions res
         JOIN knowledge_references kr ON kr.id = res.reference_id
         WHERE res.source_id = ?1
           AND res.status = 'resolved'
           AND res.target_file_id IS NOT NULL",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (from, to) = row?;
        edges.entry(from).or_default().push(to);
    }

    let mut program_roots = Vec::new();
    let mut test_roots = Vec::new();
    for entry in entries {
        let Some(target) = entry.target_file_id else {
            continue;
        };
        match entry.kind.as_str() {
            "program" | "library_root" => program_roots.push(target),
            "test" => test_roots.push(target),
            _ => {}
        }
    }
    program_roots.sort_unstable();
    program_roots.dedup();
    test_roots.sort_unstable();
    test_roots.dedup();
    Ok((
        walk(&program_roots, &edges),
        walk(&test_roots, &edges),
        program_roots,
        test_roots,
    ))
}

fn walk(roots: &[i64], edges: &HashMap<i64, Vec<i64>>) -> HashSet<i64> {
    let mut reached = HashSet::new();
    let mut queue: VecDeque<i64> = roots.iter().copied().collect();
    while let Some(file_id) = queue.pop_front() {
        if !reached.insert(file_id) {
            continue;
        }
        if let Some(targets) = edges.get(&file_id) {
            queue.extend(targets.iter().copied());
        }
    }
    reached
}

fn root_reason(prefix: &str, roots: &[i64], files: &[FileRow]) -> String {
    if roots.is_empty() {
        return prefix.to_string();
    }
    let mut paths: Vec<&str> = roots
        .iter()
        .filter_map(|id| {
            files
                .iter()
                .find(|file| file.id == *id)
                .map(|file| file.rel_path.as_str())
        })
        .collect();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {}", paths.join(", "))
    }
}

fn incomplete(files: &[FileRow]) -> bool {
    files.iter().any(|file| {
        file.reference_version < crate::knowledge::evidence::REFERENCE_INDEX_VERSION
            || blocks_conclusion(&file.symbol_status)
            || blocks_conclusion(&file.reference_status)
    })
}

fn blocks_conclusion(status: &str) -> bool {
    status != STATUS_COMPLETE && status != STATUS_UNSUPPORTED
}

fn gaps(files: &[FileRow], entries: &[EntryRow]) -> String {
    let mut lines = Vec::new();
    for file in files {
        if file.symbol_status != STATUS_COMPLETE {
            lines.push(format!("{}:{}", file.rel_path, file.symbol_status));
        } else if file.reference_status != STATUS_COMPLETE {
            lines.push(format!("{}:{}", file.rel_path, file.reference_status));
        }
        if file.reference_version < crate::knowledge::evidence::REFERENCE_INDEX_VERSION {
            lines.push(format!("{}:reference_version", file.rel_path));
        }
    }
    for entry in entries {
        if entry.target_file_id.is_none() {
            lines.push(format!("{}:missing", entry.target_rel_path));
        }
    }
    lines.sort();
    lines.dedup();
    lines.join("\n")
}

fn fingerprint(files: &[FileRow], entries: &[EntryRow]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("analysis_version:{ANALYSIS_VERSION}\n"));
    let mut files: Vec<&FileRow> = files.iter().collect();
    files.sort_by_key(|file| file.id);
    for file in files {
        hasher.update(format!(
            "file:{}:{}:{}:{}\n",
            file.id, file.content_hash, file.symbol_status, file.reference_status
        ));
    }
    let mut entries: Vec<&EntryRow> = entries.iter().collect();
    entries.sort_by(|left, right| {
        (
            &left.evidence_hash,
            &left.kind,
            &left.target_rel_path,
            left.evidence_file_id,
        )
            .cmp(&(
                &right.evidence_hash,
                &right.kind,
                &right.target_rel_path,
                right.evidence_file_id,
            ))
    });
    for entry in entries {
        hasher.update(format!(
            "entry:{}:{}:{}\n",
            entry.evidence_hash, entry.kind, entry.target_rel_path
        ));
    }
    format!("{:x}", hasher.finalize())
}

fn load_files(conn: &Connection, source_id: i64) -> Result<Vec<FileRow>> {
    let mut statement = conn.prepare(
        "SELECT id, rel_path, content_hash,
                COALESCE(symbol_status, ''),
                COALESCE(reference_status, ''),
                reference_version
         FROM knowledge_files WHERE source_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(FileRow {
            id: row.get(0)?,
            rel_path: row.get(1)?,
            content_hash: row.get(2)?,
            symbol_status: row.get(3)?,
            reference_status: row.get(4)?,
            reference_version: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_entries(conn: &Connection, source_id: i64) -> Result<Vec<EntryRow>> {
    let mut statement = conn.prepare(
        "SELECT target_file_id, target_rel_path, kind, evidence_hash, evidence_file_id
         FROM knowledge_entries WHERE source_id = ?1",
    )?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(EntryRow {
            target_file_id: row.get(0)?,
            target_rel_path: row.get(1)?,
            kind: row.get(2)?,
            evidence_hash: row.get(3)?,
            evidence_file_id: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn load_symbols(conn: &Connection, source_id: i64) -> Result<Vec<SymbolRow>> {
    let mut statement =
        conn.prepare("SELECT id, file_id, name FROM knowledge_symbols WHERE source_id = ?1")?;
    let rows = statement.query_map(params![source_id], |row| {
        Ok(SymbolRow {
            id: row.get(0)?,
            file_id: row.get(1)?,
            name: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Read the analysis row, and finding rows only when it is complete and they match it.
pub fn report(store: &KnowledgeStore, source: Option<&str>) -> Result<String> {
    let selected = resolve_source(store, source)?;
    let conn = store.conn();
    let analysis = conn
        .query_row(
            "SELECT analysis_version, evidence_generation, fingerprint, status, gaps
             FROM knowledge_analysis WHERE source_id = ?1",
            params![selected.id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((version, generation, fingerprint, status, gaps)) = analysis else {
        return Ok(format!("source: {}\nno analysis stored\n", selected.label));
    };

    let mut lines = vec![
        format!("source: {}", selected.label),
        format!("status: {status}"),
        format!("analysis_version: {version}"),
        format!("evidence_generation: {generation}"),
        format!("fingerprint: {fingerprint}"),
        "gaps:".to_string(),
    ];
    if gaps.is_empty() {
        lines.push("(none)".to_string());
    } else {
        lines.extend(gaps.lines().map(str::to_string));
    }
    if status != "complete" {
        return Ok(lines.join("\n"));
    }

    lines.push("findings:".to_string());
    let mut statement = conn.prepare(
        "SELECT f.kind, kf.rel_path, ks.name, f.reason
         FROM knowledge_findings f
         JOIN knowledge_analysis a ON a.source_id = f.source_id
         JOIN knowledge_files kf ON kf.id = f.file_id
         LEFT JOIN knowledge_symbols ks ON ks.id = f.symbol_id
         WHERE f.source_id = ?1
           AND a.status = 'complete'
           AND f.analysis_version = a.analysis_version
           AND f.evidence_generation = a.evidence_generation
           AND f.fingerprint = a.fingerprint
         ORDER BY f.kind, kf.rel_path, ks.name",
    )?;
    let rows = statement.query_map(params![selected.id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut findings = Vec::new();
    for row in rows {
        let (kind, rel_path, symbol, reason) = row?;
        let symbol = symbol.unwrap_or_else(|| "-".to_string());
        findings.push(format!("{kind} {rel_path} {symbol} {reason}"));
    }
    if findings.is_empty() {
        lines.push("none".to_string());
    } else {
        lines.extend(findings);
    }
    Ok(lines.join("\n"))
}

fn resolve_source(
    store: &KnowledgeStore,
    source: Option<&str>,
) -> Result<crate::knowledge::KnowledgeSourceInfo> {
    if let Some(label) = source.map(str::trim).filter(|label| !label.is_empty()) {
        return store
            .get_source(label)?
            .ok_or_else(|| anyhow!("knowledge source '{label}' was not found"));
    }
    let mut sources = store.list_sources()?;
    match sources.len() {
        0 => bail!("no knowledge sources are registered"),
        1 => Ok(sources.remove(0)),
        _ => {
            let labels = sources
                .iter()
                .map(|source| source.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("source label is required when more than one source is registered: {labels}")
        }
    }
}

#[cfg(test)]
mod tests;
