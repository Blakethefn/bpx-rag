use std::collections::HashMap;

use anyhow::Result;
use rusqlite::params;

pub(in crate::knowledge::sync) fn resolve_code_edges(
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
