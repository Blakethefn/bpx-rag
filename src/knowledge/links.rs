//! File and chunk relationships: wikilink and frontmatter edges between sources.

use anyhow::Result;
use rusqlite::params;

use super::{filter, KnowledgeStore, RelatedChunk};

/// Top hits that keep their rank when linked files are merged in.
pub const RELATION_KEEP_TOP: usize = 5;

/// Most linked files added from the top hit's relations.
pub const RELATION_NEIGHBOR_LIMIT: usize = 2;

impl KnowledgeStore {
    /// Get chunks from files linked to the given chunk's file.
    ///
    /// Finds the file for `chunk_rowid`, traverses graph edges in the given
    /// direction, and returns chunks from linked files.
    /// Chunks from files that link to, or are linked from, `file_id`.
    ///
    /// When `filter_predicates` is provided, only chunks whose metadata matches
    /// all predicates are returned. Predicates are ANDed.
    pub fn get_related_chunks(
        &self,
        file_id: i64,
        max_results: usize,
        filter_predicates: Option<&[filter::FilterPredicate]>,
    ) -> Result<Vec<RelatedChunk>> {
        // ?1 is file_id, so filter params start at ?2; LIMIT follows them.
        let (filter_sql, filter_params) = match filter_predicates {
            Some(preds) if !preds.is_empty() => {
                let (clause, params) = filter::predicates_to_sql(preds, 1);
                (format!(" AND {clause}"), params)
            }
            _ => (String::new(), vec![]),
        };
        let limit_idx = 2 + filter_params.len();
        let outgoing = format!(
            "SELECT kcm.file_id, kf.rel_path, kc.title, kc.content,
                    COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0)
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
                "SELECT kcm.file_id, kf.rel_path, kc.title, kc.content,
                        COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0)
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
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(RelatedChunk {
                file_id: row.get(0)?,
                rel_path: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                line_start: row.get(4)?,
                line_end: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Distinct files linked to or from `file_id`, sorted by path.
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

/// Place `neighbors` after the first `keep_top` ranked items, then the rest of
/// the ranking, without duplicates and capped at `limit`.
///
/// The top hits keep their order. Linked items fill the next slots, so a
/// relation can surface a file the ranking missed without displacing the best
/// matches. `key` identifies duplicates, such as a file path.
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
mod merge_tests {
    use super::merge_neighbors;

    #[test]
    fn neighbors_follow_the_top_hits_without_duplicates() {
        let ranked = vec!["a", "b", "c", "d", "e"];
        let neighbors = vec!["x", "a", "y"];
        let merged = merge_neighbors(ranked, neighbors, 2, 6, |item| item.to_string());
        assert_eq!(merged, vec!["a", "b", "x", "y", "c", "d"]);
    }

    #[test]
    fn neighbors_never_push_out_the_top_hits() {
        let ranked = vec!["a", "b"];
        let neighbors = vec!["x", "y", "z"];
        let merged = merge_neighbors(ranked, neighbors, 2, 2, |item| item.to_string());
        assert_eq!(merged, vec!["a", "b"]);
    }
}
