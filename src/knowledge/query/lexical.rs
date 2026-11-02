use super::*;
use crate::knowledge::identifiers::split_identifier;
use anyhow::Result;
use rusqlite::Connection;

/// BM25 search on knowledge_chunks FTS5 table.
pub(super) fn knowledge_bm25_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    // Metadata filter params come first (they use ?1, ?2, ... from predicates_to_sql)
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

    let sql = format!(
        "SELECT kc.rowid, kcm.file_id, kcm.source_id, kc.title, kc.content,
                COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                COALESCE(kcm.metadata, '{{}}'),
                COALESCE(ks.label, '') as source_label,
                COALESCE(kf.rel_path, '') as file_path,
                bm25(knowledge_chunks) as rank
         FROM knowledge_chunks kc
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE knowledge_chunks MATCH ?{fts_idx}{meta_clause}
         ORDER BY rank
         LIMIT ?{}",
        params.len() + 1
    );
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(param_refs.as_slice(), ranked_result_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

/// Decode one lexical result row. Any column that fails to decode fails the
/// whole query, so a corrupt row is reported instead of disappearing.
fn ranked_result_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RankedResult> {
    Ok(RankedResult {
        rowid: row.get(0)?,
        file_id: row.get(1)?,
        source_id: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        line_start: row.get::<_, i64>(5)? as u32,
        line_end: row.get::<_, i64>(6)? as u32,
        metadata: row.get(7)?,
        source_label: row.get(8)?,
        file_path: row.get(9)?,
        rank: row.get::<_, f64>(10)?.abs(),
    })
}

/// Trigram search on knowledge_chunks_trigram FTS5 table.
pub(super) fn knowledge_trigram_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    if query.len() < 3 {
        return Ok(Vec::new());
    }

    let fts_query = sanitize_trigram_query(query);
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

    let sql = format!(
        "SELECT kct.rowid, kcm.file_id, kcm.source_id, kct.title, kct.content,
                COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                COALESCE(kcm.metadata, '{{}}'),
                COALESCE(ks.label, '') as source_label,
                COALESCE(kf.rel_path, '') as file_path,
                bm25(knowledge_chunks_trigram) as rank
         FROM knowledge_chunks_trigram kct
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kct.rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE knowledge_chunks_trigram MATCH ?{fts_idx}{meta_clause}
         ORDER BY rank
         LIMIT ?{}",
        params.len() + 1
    );
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(param_refs.as_slice(), ranked_result_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

/// Identifier layer: BM25 over the split words of compound identifiers in the
/// `knowledge_identifiers` table. The query's own identifiers are split the same
/// way, so `getUserById` also searches for "get user by id". The content layer
/// is unchanged and still matches the exact identifier.
pub(super) fn knowledge_identifier_search(
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

    let sql = format!(
        "SELECT kc.rowid, kcm.file_id, kcm.source_id, kc.title, kc.content,
                COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                COALESCE(kcm.metadata, '{{}}'),
                COALESCE(ks.label, '') as source_label,
                COALESCE(kf.rel_path, '') as file_path,
                bm25(knowledge_identifiers) as rank
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
        .query_map(param_refs.as_slice(), ranked_result_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(results)
}

/// Words that never count toward an identifier match. Fixed in the Stage 3 protocol.
const IDENTIFIER_STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "if", "in", "into",
    "is", "it", "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where",
    "which", "with",
];
/// Only this many query words take part in identifier matching. Later words are ignored.
const IDENTIFIER_MAX_QUERY_WORDS: usize = 12;
/// A query needs at least this many content words before the identifier layer runs.
const IDENTIFIER_MIN_CONTENT_WORDS: usize = 2;

/// The identifier layer's FTS expression. Every content word of the query must appear in
/// the identifier words of one chunk. Words are split the way indexed identifiers are, so
/// `getUserById` in a query becomes "get user by id". A single content word matches nothing,
/// because one common word is too weak to identify a chunk.
pub(super) fn identifier_match_expression(query: &str) -> String {
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

/// Build an FTS5 MATCH expression: the whole query as a phrase, OR'd with each term.
pub(super) fn sanitize_fts_query(query: &str) -> String {
    let cleaned = sanitize_fts_terms(query);
    let phrase = format!("\"{cleaned}\"");
    let terms = cleaned
        .split_whitespace()
        .take(32)
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return String::new();
    }
    std::iter::once(phrase)
        .chain(terms)
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn sanitize_fts_terms(query: &str) -> String {
    let mut cleaned = String::with_capacity(query.len());
    let mut last_was_space = true;
    for character in query.chars() {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            cleaned.push(character);
            last_was_space = false;
        } else if !last_was_space {
            cleaned.push(' ');
            last_was_space = true;
        }
    }
    if cleaned.ends_with(' ') {
        cleaned.pop();
    }
    cleaned
}

/// Sanitize for trigram FTS5 MATCH (requires quoted phrase).
pub(super) fn sanitize_trigram_query(query: &str) -> String {
    let cleaned = sanitize_fts_terms(query);
    if cleaned.is_empty() {
        return String::new();
    }
    format!("\"{cleaned}\"")
}
