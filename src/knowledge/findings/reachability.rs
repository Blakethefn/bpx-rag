//! Unreachable files from a complete analysis.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::knowledge::analysis::{EntryRow, FileRow, ANALYSIS_VERSION};

pub(crate) fn write(
    conn: &Connection,
    source_id: i64,
    generation: i64,
    fingerprint: &str,
    files: &[FileRow],
    entries: &[EntryRow],
) -> Result<()> {
    let (reached, test_reached, program_roots, test_roots) =
        reachability(conn, source_id, entries)?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO knowledge_findings
            (source_id, kind, file_id, symbol_id, reason, analysis_version, evidence_generation, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

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
