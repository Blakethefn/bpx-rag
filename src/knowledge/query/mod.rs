//! Knowledge-specific search: BM25 → trigram → fuzzy → vector → RRF fusion.
//!
//! Follows the same multi-layer pipeline as `store/search.rs` but queries
//! knowledge DB tables (`knowledge_chunks`, `knowledge_chunks_trigram`, etc.)
//! instead of session tables. Written as a self-contained module to avoid
//! modifying the battle-tested session search.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

use crate::embed;
use crate::search::profile::SearchWhy;

use super::filter::{parse_filter, predicates_to_sql};
use super::KnowledgeStore;

/// Weight configuration for RRF layer blending
pub struct SearchWeights {
    pub keyword_weight: f64,
    pub vector_weight: f64,
}

impl Default for SearchWeights {
    fn default() -> Self {
        Self {
            keyword_weight: 1.0,
            vector_weight: 1.0,
        }
    }
}

/// A search result from the knowledge store (or combined search).
#[derive(Debug, serde::Serialize)]
pub struct KnowledgeSearchResult {
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub source_label: String,
    pub file_path: String,
    pub file_id: i64,
    pub source_id: i64,
    pub lines: String,
    pub metadata: serde_json::Value,
    pub source_type: String,
    pub why: SearchWhy,
}

/// Search the knowledge DB with optional metadata filtering.
///
/// Runs the full multi-layer pipeline: BM25 → trigram → fuzzy → vector → RRF.
/// Metadata filters restrict results before RRF fusion.
pub fn knowledge_search(
    store: &KnowledgeStore,
    query: &str,
    filter: Option<&str>,
    source_label: Option<&str>,
    limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<Vec<KnowledgeSearchResult>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if limit > MAX_CANDIDATE_CHUNKS {
        anyhow::bail!("knowledge search limit cannot exceed {MAX_CANDIDATE_CHUNKS}");
    }

    let conn = store.conn();
    let candidate_limit = expanded_candidate_limit(limit);

    // Parse metadata filters
    let mut predicates = parse_filter(filter)?;
    if let Some(label) = source_label {
        predicates.push(super::filter::FilterPredicate::Source(label.to_string()));
    }

    let (meta_where, meta_params) = predicates_to_sql(&predicates, 0);

    let mut all_results: HashMap<i64, ScoredResult> = HashMap::new();

    if weights.keyword_weight > 0.0 {
        // Layer 1: FTS5 BM25
        let bm25 = knowledge_bm25_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
        merge_results(&mut all_results, &bm25, weights.keyword_weight, "bm25");

        // Search a wider chunk candidate pool before choosing the final
        // result limit. File diversity is applied after chunk-level RRF.
        if bm25.len() < candidate_limit as usize {
            let trigram =
                knowledge_trigram_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
            merge_results(
                &mut all_results,
                &trigram,
                weights.keyword_weight,
                "trigram",
            );
        }

        // Layer 3: Fuzzy (if lexical layers still leave candidate room)
        if all_results.len() < candidate_limit as usize {
            let fuzzy =
                knowledge_fuzzy_search(conn, query, candidate_limit, &meta_where, &meta_params)?;
            merge_results(&mut all_results, &fuzzy, weights.keyword_weight, "fuzzy");
        }
    }

    // Layer 4: Vector similarity
    if weights.vector_weight > 0.0 {
        if let Some(emb) = store.embedder() {
            match emb
                .embed_one(query)
                .and_then(|vector| embed::normalize_embedding(&vector, emb.dim()))
            {
                Ok(query_embedding) => {
                    let candidates = store.vector_candidates(
                        emb.model_id(),
                        &query_embedding,
                        candidate_limit,
                        &predicates,
                    )?;
                    let vector = knowledge_vector_search(
                        conn,
                        &query_embedding,
                        candidate_limit,
                        &meta_where,
                        &meta_params,
                        emb.model_id(),
                        // ANN IDs come from the active model's full vector space, not
                        // lexical hits. SQL rechecks model, metadata, and exact scores.
                        candidates.as_deref(),
                    )?;
                    merge_results(&mut all_results, &vector, weights.vector_weight, "vector");
                }
                Err(e) => {
                    eprintln!(
                        "[bpx-rag] knowledge query embedding failed, skipping vector layer: {e}"
                    );
                }
            }
        }
    }

    // Collect and sort by RRF score
    let mut results: Vec<KnowledgeSearchResult> = all_results
        .into_values()
        .map(|sr| sr.into_knowledge_result(snippet_bytes))
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(diversify_by_file(results, limit as usize))
}

const MAX_CANDIDATE_CHUNKS: u32 = 1_000;
pub const CANDIDATE_OVERSAMPLE_FACTOR: u32 = 5;

fn expanded_candidate_limit(limit: u32) -> u32 {
    limit
        .saturating_mul(CANDIDATE_OVERSAMPLE_FACTOR)
        .min(MAX_CANDIDATE_CHUNKS)
        .max(limit)
}

/// Keep the highest-scoring passage from each file before returning repeated
/// files. If fewer files than `limit` are available, duplicate passages fill
/// the remaining slots in score order; in that fallback case, output order is
/// file-diversity-first rather than globally score-descending.
fn diversify_by_file(
    results: Vec<KnowledgeSearchResult>,
    limit: usize,
) -> Vec<KnowledgeSearchResult> {
    let mut seen_file_ids = std::collections::HashSet::new();
    let mut unique_files = Vec::with_capacity(limit);
    let mut duplicate_chunks = Vec::new();
    for result in results {
        if seen_file_ids.insert(result.file_id) {
            unique_files.push(result);
            if unique_files.len() == limit {
                return unique_files;
            }
        } else {
            duplicate_chunks.push(result);
        }
    }
    unique_files.extend(
        duplicate_chunks
            .into_iter()
            .take(limit - unique_files.len()),
    );
    unique_files
}

/// Search both session and knowledge DBs, merging results via RRF.
#[derive(Debug)]
struct RankedResult {
    rowid: i64,
    file_id: i64,
    source_id: i64,
    title: String,
    content: String,
    rank: f64,
    source_label: String,
    file_path: String,
    line_start: u32,
    line_end: u32,
    metadata: String,
}

struct ScoredResult {
    file_id: i64,
    source_id: i64,
    title: String,
    content: String,
    source_label: String,
    file_path: String,
    line_start: u32,
    line_end: u32,
    metadata: String,
    rrf_score: f64,
    why: SearchWhy,
}

impl ScoredResult {
    fn into_knowledge_result(self, snippet_bytes: usize) -> KnowledgeSearchResult {
        let metadata: serde_json::Value = serde_json::from_str(&self.metadata)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        KnowledgeSearchResult {
            title: self.title,
            snippet: truncate_snippet(&self.content, snippet_bytes),
            score: self.rrf_score,
            source_label: self.source_label,
            file_path: self.file_path,
            file_id: self.file_id,
            source_id: self.source_id,
            lines: format!("{}-{}", self.line_start, self.line_end),
            metadata,
            source_type: "knowledge".to_string(),
            why: self.why,
        }
    }
}

impl Clone for KnowledgeSearchResult {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            snippet: self.snippet.clone(),
            score: self.score,
            source_label: self.source_label.clone(),
            file_path: self.file_path.clone(),
            file_id: self.file_id,
            source_id: self.source_id,
            lines: self.lines.clone(),
            metadata: self.metadata.clone(),
            source_type: self.source_type.clone(),
            why: self.why.clone(),
        }
    }
}

// ── Search layers ────────────────────────────────────────────────────────────

/// BM25 search on knowledge_chunks FTS5 table.
fn knowledge_bm25_search(
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
        .query_map(param_refs.as_slice(), |row| {
            Ok(RankedResult {
                rowid: row.get(0)?,
                file_id: row.get(1)?,
                source_id: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                line_start: row.get::<_, i64>(5).unwrap_or(0) as u32,
                line_end: row.get::<_, i64>(6).unwrap_or(0) as u32,
                metadata: row.get(7)?,
                source_label: row.get(8)?,
                file_path: row.get(9)?,
                rank: row.get::<_, f64>(10)?.abs(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Trigram search on knowledge_chunks_trigram FTS5 table.
fn knowledge_trigram_search(
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
        .query_map(param_refs.as_slice(), |row| {
            Ok(RankedResult {
                rowid: row.get(0)?,
                file_id: row.get(1)?,
                source_id: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                line_start: row.get::<_, i64>(5).unwrap_or(0) as u32,
                line_end: row.get::<_, i64>(6).unwrap_or(0) as u32,
                metadata: row.get(7)?,
                source_label: row.get(8)?,
                file_path: row.get(9)?,
                rank: row.get::<_, f64>(10)?.abs(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Levenshtein fuzzy search: correct query terms against knowledge index vocab.
fn knowledge_fuzzy_search(
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
fn extract_knowledge_terms(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    let result = conn
        .prepare("SELECT DISTINCT term FROM knowledge_chunks_vocab WHERE col = 'content' LIMIT ?1");

    match result {
        Ok(mut stmt) => {
            let terms = stmt
                .query_map(rusqlite::params![limit as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(terms)
        }
        Err(_) => {
            // Vocab table may not exist; create it and retry
            let _ = conn.execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab \
                 USING fts5vocab(knowledge_chunks, instance);",
            );
            let mut stmt = conn.prepare(
                "SELECT DISTINCT term FROM knowledge_chunks_vocab WHERE col = 'content' LIMIT ?1",
            )?;
            let terms = stmt
                .query_map(rusqlite::params![limit as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(terms)
        }
    }
}

/// Vector similarity search over knowledge embeddings.
fn knowledge_vector_search(
    conn: &Connection,
    query_embedding: &[f32],
    limit: u32,
    meta_where: &str,
    meta_params: &[String],
    model_id: &str,
    candidate_rowids: Option<&[i64]>,
) -> Result<Vec<RankedResult>> {
    if candidate_rowids.is_some_and(|rowids| rowids.is_empty()) || limit == 0 {
        return Ok(Vec::new());
    }
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    // Metadata filter params
    for p in meta_params {
        params.push(Box::new(p.clone()));
    }

    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };

    let mut sql = format!(
        "SELECT ke.chunk_rowid, ke.embedding,
                kcm.file_id, kcm.source_id,
                kc.title, kc.content,
                COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                COALESCE(kcm.metadata, '{{}}'),
                COALESCE(ks.label, '') as source_label,
                COALESCE(kf.rel_path, '') as file_path
         FROM knowledge_embeddings ke
         JOIN knowledge_chunks kc ON kc.rowid = ke.chunk_rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE 1=1{meta_clause}"
    );

    let model_param_index = params.len() + 1;
    sql.push_str(&format!(" AND ke.model_id = ?{model_param_index}"));
    params.push(Box::new(model_id.to_string()));

    if let Some(rowids) = candidate_rowids {
        if !rowids.is_empty() {
            let placeholders: Vec<String> = rowids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + i + 1))
                .collect();
            sql.push_str(&format!(
                " AND ke.chunk_rowid IN ({})",
                placeholders.join(",")
            ));
            for &rid in rowids {
                params.push(Box::new(rid));
            }
        }
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let mut scored: Vec<(f32, RankedResult)> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let chunk_rowid: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((
                blob,
                chunk_rowid,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6).unwrap_or(0) as u32,
                row.get::<_, i64>(7).unwrap_or(0) as u32,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(blob, rowid, file_id, source_id, title, content, ls, le, meta, source, path)| {
                let embedding = embed::bytes_to_embedding(&blob);
                let similarity = dot_product(query_embedding, &embedding);
                (
                    similarity,
                    RankedResult {
                        rowid,
                        file_id,
                        source_id,
                        title,
                        content,
                        rank: 0.0,
                        source_label: source,
                        file_path: path,
                        line_start: ls,
                        line_end: le,
                        metadata: meta,
                    },
                )
            },
        )
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit as usize);

    Ok(scored
        .into_iter()
        .enumerate()
        .map(|(rank, (_, mut result))| {
            result.rank = rank as f64;
            result
        })
        .collect())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// RRF merge with K=60, matching session search behavior.
fn merge_results(
    acc: &mut HashMap<i64, ScoredResult>,
    results: &[RankedResult],
    weight: f64,
    layer: &str,
) {
    const K: f64 = 60.0;

    for (rank, result) in results.iter().enumerate() {
        let rrf_score = weight * (1.0 / (K + rank as f64 + 1.0));

        acc.entry(result.rowid)
            .and_modify(|sr| {
                sr.rrf_score += rrf_score;
                match layer {
                    "bm25" => sr.why.layers.bm25 += rrf_score,
                    "trigram" => sr.why.layers.trigram += rrf_score,
                    "fuzzy" => sr.why.layers.fuzzy += rrf_score,
                    "vector" => sr.why.layers.vector += rrf_score,
                    _ => {}
                }
            })
            .or_insert_with(|| ScoredResult {
                file_id: result.file_id,
                source_id: result.source_id,
                title: result.title.clone(),
                content: result.content.clone(),
                source_label: result.source_label.clone(),
                file_path: result.file_path.clone(),
                line_start: result.line_start,
                line_end: result.line_end,
                metadata: result.metadata.clone(),
                rrf_score,
                why: {
                    let mut why = SearchWhy::default();
                    match layer {
                        "bm25" => why.layers.bm25 = rrf_score,
                        "trigram" => why.layers.trigram = rrf_score,
                        "fuzzy" => why.layers.fuzzy = rrf_score,
                        "vector" => why.layers.vector = rrf_score,
                        _ => {}
                    }
                    why
                },
            });
    }
}

/// Dot product for cosine similarity (vectors are L2-normalized at index time).
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Truncate content to a snippet at word boundaries.
fn truncate_snippet(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    // Keep the byte limit while always slicing on a valid UTF-8 boundary.
    let boundary = content
        .char_indices()
        .take_while(|(index, _)| *index <= max_bytes)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    let truncated = &content[..boundary];
    let cut = truncated
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map(|(index, _)| index)
        .filter(|index| *index > 0)
        .unwrap_or(boundary);
    format!("{}...", &content[..cut])
}

/// Convert a session SearchResult to a KnowledgeSearchResult.
fn sanitize_fts_query(query: &str) -> String {
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

fn sanitize_fts_terms(query: &str) -> String {
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
fn sanitize_trigram_query(query: &str) -> String {
    let cleaned = sanitize_fts_terms(query);
    if cleaned.is_empty() {
        return String::new();
    }
    format!("\"{cleaned}\"")
}

#[cfg(test)]
mod tests;
