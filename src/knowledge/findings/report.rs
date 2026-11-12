use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};

use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

const PUBLISHED_STATUSES: &[&str] = &["complete", "partial"];

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
    if !PUBLISHED_STATUSES.contains(&status.as_str()) {
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

fn resolve_source(store: &KnowledgeStore, source: Option<&str>) -> Result<KnowledgeSourceInfo> {
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
