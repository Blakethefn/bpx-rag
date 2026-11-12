use std::collections::HashMap;

use anyhow::{Context, Result};
use rusqlite::params;

/// Resolve an Obsidian wikilink target to a file ID.
///
/// Resolution order:
/// 1. Exact match on rel_path
/// 2. Exact match with `.md` appended
/// 3. Basename match (target matches the filename portion of any rel_path)
pub(in crate::knowledge::sync) fn resolve_wikilink(
    target: &str,
    files: &HashMap<String, i64>,
) -> Option<i64> {
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
pub(in crate::knowledge::sync) fn resolve_wikilinks_for_source(
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
