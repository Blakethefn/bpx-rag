use anyhow::Result;
use rusqlite::Connection;

use crate::knowledge::identifiers::split_identifier;

use super::super::rank::RankedResult;
use super::{lexical_columns, ranked_result_from_row, select_list};

pub(in crate::knowledge::query) fn knowledge_identifier_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    let fts_query = identifier_match_expression(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let fts_idx = params.len() + 1;
    params.push(Box::new(fts_query));

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let columns = lexical_columns("kc", "bm25(knowledge_identifiers)");
    let select = select_list(&columns);
    let sql = format!(
        "SELECT {select}
         FROM knowledge_identifiers
         JOIN knowledge_chunks kc ON kc.rowid = knowledge_identifiers.rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = knowledge_identifiers.rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE knowledge_identifiers MATCH ?{fts_idx}{meta_clause}
         ORDER BY rank
         LIMIT ?{}",
        params.len() + 1
    );
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(param_refs.as_slice(), |row| {
            ranked_result_from_row(row, &columns)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

const IDENTIFIER_STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "if", "in", "into",
    "is", "it", "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where",
    "which", "with",
];
const IDENTIFIER_MAX_QUERY_WORDS: usize = 12;
const IDENTIFIER_MIN_CONTENT_WORDS: usize = 2;

pub(in crate::knowledge::query) fn identifier_match_expression(query: &str) -> String {
    let words: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .flat_map(split_identifier)
        .take(IDENTIFIER_MAX_QUERY_WORDS)
        .filter(|word| !IDENTIFIER_STOPWORDS.contains(&word.as_str()))
        .collect();
    if words.len() < IDENTIFIER_MIN_CONTENT_WORDS {
        return String::new();
    }
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(" AND ")
}
