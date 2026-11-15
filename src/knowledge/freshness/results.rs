use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::super::query::KnowledgeSearchResult;
use super::super::KnowledgeStore;
use super::{check_indexed_file, now_ns, unknown, Freshness, FreshnessBasis, IndexedFile};

pub fn check_results(
    store: &KnowledgeStore,
    results: &[KnowledgeSearchResult],
) -> Result<Vec<Freshness>> {
    let now = now_ns();
    let mut source_paths: HashMap<i64, Option<String>> = HashMap::new();
    let mut checks = Vec::with_capacity(results.len());
    for result in results {
        let base = match source_paths.get(&result.source_id) {
            Some(path) => path.clone(),
            None => {
                let path = store
                    .conn()
                    .query_row(
                        "SELECT path FROM knowledge_sources WHERE id = ?1",
                        params![result.source_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                source_paths.insert(result.source_id, path.clone());
                path
            }
        };
        let Some(base) = base else {
            checks.push(unknown(FreshnessBasis::SourceUnavailable));
            continue;
        };
        let record = indexed_file(store, result.file_id)?;
        checks.push(match record {
            Some(record) => check_indexed_file(Path::new(&base), &record, now),
            None => unknown(FreshnessBasis::NotRecorded),
        });
    }
    Ok(checks)
}

fn indexed_file(store: &KnowledgeStore, file_id: i64) -> Result<Option<IndexedFile>> {
    let record = store
        .conn()
        .query_row(
            "SELECT rel_path, content_hash, size_bytes, mtime_ns, stat_checked_ns
             FROM knowledge_files WHERE id = ?1",
            params![file_id],
            |row| {
                Ok(IndexedFile {
                    rel_path: row.get(0)?,
                    content_hash: row.get(1)?,
                    size_bytes: row.get(2)?,
                    mtime_ns: row.get(3)?,
                    stat_checked_ns: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(record)
}
