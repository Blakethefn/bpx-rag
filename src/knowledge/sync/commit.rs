use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use rusqlite::params;

use crate::knowledge::KnowledgeSourceInfo;

use super::classify::ChangePlan;
use super::files::{load_db_files, DbFile, PreparedFile};
use super::index::{
    delete_file_chunks, index_prepared_file, replace_file_evidence, resolve_code_edges,
    resolve_wikilinks_for_source, FileEvidence,
};

pub(super) fn commit_plan(
    conn: &rusqlite::Connection,
    source: &KnowledgeSourceInfo,
    db_files: &HashMap<String, DbFile>,
    plan: &ChangePlan,
    prepared_files: &[PreparedFile],
    model_id: Option<&str>,
    changed: bool,
) -> Result<(usize, u64)> {
    let transaction_start = Instant::now();
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let inner = (|| -> Result<usize> {
        if &load_db_files(conn, source.id)? != db_files {
            anyhow::bail!("knowledge source changed during sync preparation; retry sync");
        }

        for file_id in &plan.to_remove {
            delete_file_chunks(conn, *file_id)?;
            conn.execute(
                "DELETE FROM knowledge_files WHERE id = ?1",
                params![file_id],
            )?;
        }

        for file in &plan.backfill {
            replace_file_evidence(
                conn,
                FileEvidence {
                    source_id: source.id,
                    file_id: file.file_id,
                    rel_path: &file.rel_path,
                    status: file.status,
                    symbols: &file.symbols,
                    references: &file.references,
                    entries: &file.entries,
                },
            )?;
        }
        for (file_id, stat) in &plan.backfill_stat {
            conn.execute(
                "UPDATE knowledge_files SET mtime_ns = ?1, stat_checked_ns = ?2 WHERE id = ?3",
                params![stat.mtime_ns, stat.checked_ns, file_id],
            )?;
        }

        for file in prepared_files {
            let file_id = match file.file_id {
                Some(file_id) => {
                    delete_file_chunks(conn, file_id)?;
                    file_id
                }
                None => {
                    conn.execute(
                        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![source.id, file.rel_path, file.hash, file.size_bytes],
                    )?;
                    conn.last_insert_rowid()
                }
            };
            index_prepared_file(conn, source.id, file_id, file, model_id)?;
        }

        conn.execute(
            "UPDATE knowledge_sources SET
                last_sync = datetime('now'),
                file_count = (SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1),
                chunk_count = (SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1)
             WHERE id = ?1",
            params![source.id],
        )?;

        crate::knowledge::identifiers::index_pending(conn)?;

        let lc = resolve_wikilinks_for_source(conn, source.id)?;
        let has_code_edges: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM knowledge_links kl
             JOIN knowledge_files kf ON kf.id = kl.source_file_id
             WHERE kf.source_id = ?1 AND kl.link_type LIKE 'code:%')",
            params![source.id],
            |row| row.get(0),
        )?;
        if changed || !has_code_edges {
            let root = Path::new(&source.path).canonicalize()?;
            resolve_code_edges(conn, source.id, &root)?;
        }

        let structural = !plan.to_add.is_empty()
            || !plan.to_update.is_empty()
            || !plan.to_remove.is_empty()
            || !plan.backfill.is_empty();
        if structural {
            crate::knowledge::entries::refresh_implicit(conn, source.id, Path::new(&source.path))?;
        }
        crate::knowledge::rebuild::rebuild_source(conn, source.id, structural)?;

        Ok(lc)
    })();

    let links_created = match inner {
        Ok(links_created) => {
            conn.execute_batch("COMMIT")?;
            links_created
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };
    Ok((
        links_created,
        transaction_start.elapsed().as_millis() as u64,
    ))
}
