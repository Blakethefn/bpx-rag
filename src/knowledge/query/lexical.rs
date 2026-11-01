use super::*;
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

/// Levenshtein fuzzy search: correct query terms against knowledge index vocab.
pub(super) fn knowledge_fuzzy_search(
    conn: &Connection,
    query: &str,
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
) -> Result<Vec<RankedResult>> {
    let terms = extract_knowledge_terms(conn, 500)?;
    let query_words: Vec<&str> = query.split_whitespace().collect();

    let mut corrected_words = Vec::new();
    for word in &query_words {
        let word_lower = word.to_lowercase();
        let mut best_match = word.to_string();
        let mut best_distance = 3usize;

        for term in &terms {
            let dist = strsim::levenshtein(&word_lower, term);
            if dist < best_distance && dist <= 2 {
                best_distance = dist;
                best_match = term.clone();
            }
        }
        corrected_words.push(best_match);
    }

    let corrected_query = corrected_words.join(" ");
    if corrected_query == query {
        return Ok(Vec::new());
    }

    knowledge_bm25_search(conn, &corrected_query, limit, meta_where, meta_params)
}

/// Extract terms from the knowledge FTS5 vocab for fuzzy matching.
///
/// The vocab table is created on first use for databases that predate it.
/// Creation and query errors are returned, not treated as a missing table.
pub(super) fn extract_knowledge_terms(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab \
         USING fts5vocab(knowledge_chunks, instance);",
    )?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT term FROM knowledge_chunks_vocab WHERE col = 'content' ORDER BY term LIMIT ?1",
    )?;
    let terms = stmt
        .query_map(rusqlite::params![limit as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(terms)
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
