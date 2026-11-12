use anyhow::Result;
use rusqlite::params;

use super::{filter, KnowledgeStore, RelatedChunk};

pub const RELATION_KEEP_TOP: usize = 5;

pub const RELATION_NEIGHBOR_LIMIT: usize = 2;

const RELATED_COLUMNS: &[&str] = &[
    "kcm.file_id",
    "kf.rel_path",
    "kc.title",
    "kc.content",
    "COALESCE(kcm.line_start, 0)",
    "COALESCE(kcm.line_end, 0)",
];

fn related_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelatedChunk> {
    Ok(RelatedChunk {
        file_id: row.get(0)?,
        rel_path: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        line_start: row.get(4)?,
        line_end: row.get(5)?,
    })
}

impl KnowledgeStore {
    pub fn get_related_chunks(
        &self,
        file_id: i64,
        max_results: usize,
        filter_predicates: Option<&[filter::FilterPredicate]>,
    ) -> Result<Vec<RelatedChunk>> {
        let (filter_sql, filter_params) = match filter_predicates {
            Some(preds) if !preds.is_empty() => {
                let (clause, params) = filter::predicates_to_sql(preds, 1);
                (format!(" AND {clause}"), params)
            }
            _ => (String::new(), vec![]),
        };
        let limit_idx = 2 + filter_params.len();
        let columns = RELATED_COLUMNS.join(", ");
        let outgoing = format!(
            "SELECT {columns}
             FROM knowledge_links kl
             JOIN knowledge_chunk_meta kcm ON kcm.file_id = kl.target_file_id
             JOIN knowledge_chunks kc ON kc.rowid = kcm.chunk_rowid
             JOIN knowledge_files kf ON kf.id = kl.target_file_id
             WHERE kl.source_file_id = ?1{filter_sql}
             LIMIT ?{limit_idx}"
        );
        let mut results = self.query_related(&outgoing, file_id, &filter_params, max_results)?;

        let remaining = max_results.saturating_sub(results.len());
        if remaining > 0 {
            let incoming = format!(
                "SELECT {columns}
                 FROM knowledge_links kl
                 JOIN knowledge_chunk_meta kcm ON kcm.file_id = kl.source_file_id
                 JOIN knowledge_chunks kc ON kc.rowid = kcm.chunk_rowid
                 JOIN knowledge_files kf ON kf.id = kl.source_file_id
                 WHERE kl.target_file_id = ?1{filter_sql}
                 LIMIT ?{limit_idx}"
            );
            results.extend(self.query_related(&incoming, file_id, &filter_params, remaining)?);
        }
        Ok(results)
    }

    fn query_related(
        &self,
        sql: &str,
        file_id: i64,
        filter_params: &[String],
        limit: usize,
    ) -> Result<Vec<RelatedChunk>> {
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(file_id)];
        for param in filter_params {
            params.push(Box::new(param.clone()));
        }
        params.push(Box::new(limit as i64));
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(refs.as_slice(), related_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn related_files(&self, file_id: i64, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT kf.rel_path
             FROM knowledge_links kl
             JOIN knowledge_files kf ON kf.id = CASE
                 WHEN kl.source_file_id = ?1 THEN kl.target_file_id
                 ELSE kl.source_file_id
             END
             WHERE (kl.source_file_id = ?1 OR kl.target_file_id = ?1)
               AND kf.id != ?1
             ORDER BY kf.rel_path
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![file_id, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

pub fn merge_neighbors<T, K: PartialEq>(
    ranked: Vec<T>,
    neighbors: Vec<T>,
    keep_top: usize,
    limit: usize,
    key: impl Fn(&T) -> K,
) -> Vec<T> {
    let mut tail = ranked;
    let split = keep_top.min(tail.len());
    let rest = tail.split_off(split);
    let mut merged = tail;
    for neighbor in neighbors {
        if merged.len() >= limit {
            break;
        }
        let neighbor_key = key(&neighbor);
        let duplicate = merged
            .iter()
            .chain(rest.iter())
            .any(|item| key(item) == neighbor_key);
        if !duplicate {
            merged.push(neighbor);
        }
    }
    for item in rest {
        if merged.len() >= limit {
            break;
        }
        merged.push(item);
    }
    merged
}

#[cfg(test)]
mod tests;
