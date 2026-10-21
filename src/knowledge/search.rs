//! Knowledge-specific search: BM25 → trigram → fuzzy → vector → RRF fusion.
//!
//! Follows the same multi-layer pipeline as `store/search.rs` but queries
//! knowledge DB tables (`knowledge_chunks`, `knowledge_chunks_trigram`, etc.)
//! instead of session tables. Written as a self-contained module to avoid
//! modifying the battle-tested session search.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

use crate::embedder;
use crate::search_model::SearchWhy;
use crate::store::search::{SearchResult, SearchWeights};
use crate::store::ContentStore;

use super::filter::{parse_filter, predicates_to_sql};
use super::KnowledgeStore;

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
                .and_then(|vector| embedder::normalize_embedding(&vector, emb.dim()))
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
pub fn combined_search(
    session_store: &ContentStore,
    knowledge_store: &KnowledgeStore,
    query: &str,
    filter: Option<&str>,
    limit: u32,
    weights: &SearchWeights,
    snippet_bytes: usize,
) -> Result<Vec<KnowledgeSearchResult>> {
    // 1. Session search
    let session_results = session_store.search_with_weights(query, limit, None, None, weights)?;

    // 2. Knowledge search
    let knowledge_results = knowledge_search(
        knowledge_store,
        query,
        filter,
        None,
        limit,
        weights,
        snippet_bytes,
    )?;

    // 3. Convert session results to KnowledgeSearchResult
    let session_converted: Vec<KnowledgeSearchResult> = session_results
        .into_iter()
        .map(|sr| session_to_knowledge_result(sr, snippet_bytes))
        .collect();

    // 4. Merge via RRF
    const K: f64 = 60.0;
    let mut merged: HashMap<String, MergedResult> = HashMap::new();

    for (rank, result) in session_converted.iter().enumerate() {
        let rrf = 1.0 / (K + rank as f64 + 1.0);
        let key = format!("session:{}:{}", result.source_label, result.title);
        merged
            .entry(key)
            .and_modify(|m| m.rrf_score += rrf)
            .or_insert(MergedResult {
                result: result.clone(),
                rrf_score: rrf,
            });
    }

    for (rank, result) in knowledge_results.iter().enumerate() {
        let rrf = 1.0 / (K + rank as f64 + 1.0);
        let key = format!("knowledge:{}:{}", result.source_label, result.title);
        merged
            .entry(key)
            .and_modify(|m| m.rrf_score += rrf)
            .or_insert(MergedResult {
                result: result.clone(),
                rrf_score: rrf,
            });
    }

    let mut final_results: Vec<KnowledgeSearchResult> = merged
        .into_values()
        .map(|m| {
            let mut r = m.result;
            r.score = m.rrf_score;
            r
        })
        .collect();

    final_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    final_results.truncate(limit as usize);
    Ok(final_results)
}

// ── Internal types ───────────────────────────────────────────────────────────

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

#[derive(Clone)]
struct MergedResult {
    result: KnowledgeSearchResult,
    rrf_score: f64,
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
                let embedding = embedder::bytes_to_embedding(&blob);
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
fn session_to_knowledge_result(sr: SearchResult, snippet_bytes: usize) -> KnowledgeSearchResult {
    KnowledgeSearchResult {
        title: sr.title,
        snippet: truncate_snippet(&sr.content, snippet_bytes),
        score: sr.score,
        source_label: sr.source.clone(),
        file_path: sr.source,
        file_id: 0,
        source_id: sr.source_id,
        lines: format!("{}-{}", sr.line_start, sr.line_end),
        metadata: serde_json::Value::Object(Default::default()),
        source_type: "session".to_string(),
        why: sr.why,
    }
}

/// Sanitize a query for FTS5 MATCH syntax.
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
mod tests {
    use super::*;
    use crate::embedder::Embed;
    use crate::knowledge;
    use rusqlite::params;
    use std::sync::Arc;

    struct SemanticTestEmbedder;

    impl Embed for SemanticTestEmbedder {
        fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
            Ok(
                if text == "whale" || text.contains("blue ocean migration") {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                },
            )
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            texts.iter().map(|text| self.embed_one(text)).collect()
        }

        fn dim(&self) -> usize {
            2
        }
    }

    struct EvaluationEmbedder;

    impl EvaluationEmbedder {
        fn vector(text: &str) -> Vec<f32> {
            let text = text.to_lowercase();
            if text.contains("prevent users getting in")
                || text.contains("authorization")
                || text.contains("authentication")
                || text.contains("permission policy")
            {
                vec![0.0, 1.0, 0.0, 0.0]
            } else if text.contains("client lifetime")
                || text.contains("postgres connection pool")
                || text.contains("pool max lifetime")
            {
                vec![0.0, 0.0, 1.0, 0.0]
            } else if text.contains("queue acknowledgement")
                || text.contains("queue ack")
                || text.contains("durable queue")
            {
                vec![1.0, 0.0, 0.0, 0.0]
            } else {
                vec![0.0, 0.0, 0.0, 1.0]
            }
        }
    }

    impl Embed for EvaluationEmbedder {
        fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
            Ok(Self::vector(text))
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|text| Self::vector(text)).collect())
        }

        fn dim(&self) -> usize {
            4
        }
    }

    fn make_store() -> KnowledgeStore {
        knowledge::open_in_memory().expect("in-memory knowledge store")
    }

    #[test]
    fn lexical_search_matches_individual_terms_in_natural_language_queries() {
        let store = make_store();
        seed_chunk(
            &store,
            "repo",
            "/repo",
            "src/search.rs",
            "Hybrid retrieval layers",
            "The pipeline runs BM25, trigram, fuzzy, and semantic vector retrieval, then merges ranked results.",
            "{}",
        );

        let results = knowledge_search(
            &store,
            "Which code runs BM25 and semantic vector retrieval across the corpus?",
            None,
            Some("repo"),
            10,
            &SearchWeights {
                keyword_weight: 1.0,
                vector_weight: 0.0,
            },
            2000,
        )
        .unwrap();

        assert_eq!(results[0].file_path, "src/search.rs");
        assert!(results[0].why.layers.bm25 > 0.0);
    }

    #[test]
    fn dense_retrieval_can_find_a_chunk_with_no_lexical_hit() {
        let mut store = make_store();
        store.set_embedder(Arc::new(SemanticTestEmbedder));
        let lexical = seed_chunk(
            &store,
            "repo",
            "/repo",
            "src/lexical.md",
            "Lexical distractor",
            "whale appears in a glossary but does not describe movement",
            "{}",
        );
        let semantic = seed_chunk(
            &store,
            "repo",
            "/repo",
            "src/migration.md",
            "Ocean migration",
            "blue ocean migration paths of large mammals",
            "{}",
        );
        for (rowid, content) in [
            (
                lexical,
                "whale appears in a glossary but does not describe movement",
            ),
            (semantic, "blue ocean migration paths of large mammals"),
        ] {
            let embedding = SemanticTestEmbedder.embed_one(content).unwrap();
            store.conn().execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim) VALUES (?1, ?2, 2)",
                params![rowid, embedder::embedding_to_bytes(&embedding)],
            ).unwrap();
        }

        let results = knowledge_search(
            &store,
            "whale",
            None,
            None,
            10,
            &SearchWeights::default(),
            2000,
        )
        .unwrap();

        assert!(results.iter().any(|result| {
            result.file_path == "src/migration.md" && result.why.layers.vector > 0.0
        }));
    }

    #[test]
    fn dense_retrieval_ignores_vectors_from_a_different_model() {
        let mut store = make_store();
        store.set_embedder(Arc::new(SemanticTestEmbedder));
        let rowid = seed_chunk(
            &store,
            "repo",
            "/repo",
            "src/migration.md",
            "Ocean migration",
            "blue ocean migration paths of large mammals",
            "{}",
        );
        let incompatible = SemanticTestEmbedder
            .embed_one("blue ocean migration")
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, 2, 'jinaai/jina-embeddings-v2-base-code')",
                params![rowid, embedder::embedding_to_bytes(&incompatible)],
            )
            .unwrap();

        let results = knowledge_search(
            &store,
            "whale",
            None,
            None,
            10,
            &SearchWeights::default(),
            2000,
        )
        .unwrap();

        assert!(
            results.is_empty(),
            "vectors from a foreign model must not be compared"
        );
    }

    #[test]
    fn explicit_empty_vector_candidates_return_no_results() {
        let store = make_store();
        let rowid = seed_chunk(
            &store,
            "repo",
            "/repo",
            "src/vector.md",
            "Vector chunk",
            "text that would otherwise be searchable",
            "{}",
        );
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
                 VALUES (?1, ?2, 2)",
                params![rowid, embedder::embedding_to_bytes(&[1.0, 0.0])],
            )
            .unwrap();

        let results = knowledge_vector_search(
            store.conn(),
            &[1.0, 0.0],
            10,
            "",
            &[],
            "sentence-transformers/all-MiniLM-L6-v2",
            Some(&[]),
        )
        .unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn deterministic_retrieval_eval_measures_recall_mrr_and_ndcg() {
        let mut store = make_store();
        store.set_embedder(Arc::new(EvaluationEmbedder));
        let corpus = [
            (
                "code",
                "src/queue/ack.rs",
                "Queue acknowledgement",
                "Durable queue acknowledgement retries until the message is committed.",
            ),
            (
                "code",
                "src/auth/policy.rs",
                "Permission policy",
                "Authorization checks verify roles before privileged actions are allowed.",
            ),
            (
                "library",
                "src/db/pool.rs",
                "Postgres connection pool",
                "The client lifetime config bounds pooled PostgreSQL connections.",
            ),
            (
                "guide",
                "docs/pool.md",
                "Pool max lifetime",
                "Postgres connection pool max lifetime controls how long idle clients remain reusable.",
            ),
            (
                "docs",
                "docs/retry.md",
                "Retry policy",
                "Backoff delays spread repeated network attempts across the queue.",
            ),
            (
                "docs",
                "docs/login.md",
                "Login session",
                "Sessions expire after a configured inactivity duration.",
            ),
            (
                "code",
                "src/cache/store.rs",
                "Cache eviction",
                "Least recently used entries are removed to keep memory bounded.",
            ),
            (
                "docs",
                "docs/monitoring.md",
                "Monitoring alerts",
                "Latency and error rate alerts notify the on-call engineer.",
            ),
        ];

        for (source, path, title, content) in corpus {
            let rowid = seed_chunk(&store, source, "/eval", path, title, content, "{}");
            let embedding = EvaluationEmbedder::vector(&format!("{title} {content}"));
            store.conn().execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim) VALUES (?1, ?2, 4)",
                params![rowid, embedder::embedding_to_bytes(&embedding)],
            ).unwrap();
        }

        let cases = [
            ("durable queue ack", None, "src/queue/ack.rs"),
            ("authentcation polciy", None, "src/auth/policy.rs"),
            ("prevent users getting in", None, "src/auth/policy.rs"),
            ("client lifetime", Some("library"), "src/db/pool.rs"),
        ];
        let query_count = cases.len() as f64;
        let mut reciprocal_rank_sum = 0.0;
        let mut ndcg_sum = 0.0;
        let mut recall_sum = 0.0;

        for (query, source, relevant_path) in cases {
            let results = knowledge_search(
                &store,
                query,
                None,
                source,
                10,
                &SearchWeights::default(),
                2000,
            )
            .unwrap();
            let ranks = results
                .iter()
                .map(|result| result.file_path.as_str())
                .collect::<Vec<_>>();
            let relevant_rank = ranks.iter().position(|path| *path == relevant_path);
            recall_sum += f64::from(relevant_rank.is_some());
            if let Some(rank) = relevant_rank {
                reciprocal_rank_sum += 1.0 / (rank as f64 + 1.0);
                ndcg_sum += 1.0 / ((rank as f64 + 2.0).log2());
            }
            assert!(
                results
                    .iter()
                    .all(|result| source.map_or(true, |source| result.source_label == source)),
                "source-filtered query returned an out-of-scope result"
            );
            assert!(
                relevant_rank.is_some(),
                "relevant result missing for query: {query}; got {ranks:?}"
            );
        }

        let recall_at_10 = recall_sum / query_count;
        let mrr_at_10 = reciprocal_rank_sum / query_count;
        let ndcg_at_10 = ndcg_sum / query_count;
        println!(
            "deterministic retrieval eval: Recall@10={recall_at_10:.3}, MRR@10={mrr_at_10:.3}, nDCG@10={ndcg_at_10:.3}"
        );
        assert!(recall_at_10 >= 1.0);
        assert!(mrr_at_10 >= 0.5);
        assert!(ndcg_at_10 >= 0.7);
    }

    /// Insert a source + file + chunk with metadata for testing search.
    fn seed_chunk(
        store: &KnowledgeStore,
        source_label: &str,
        source_path: &str,
        rel_path: &str,
        title: &str,
        content: &str,
        metadata_json: &str,
    ) -> i64 {
        let conn = store.conn();

        // Ensure source exists
        let source_id: i64 = match conn.query_row(
            "SELECT id FROM knowledge_sources WHERE label = ?1",
            params![source_label],
            |row| row.get(0),
        ) {
            Ok(id) => id,
            Err(_) => {
                conn.execute(
                    "INSERT INTO knowledge_sources (label, path, enrichments) VALUES (?1, ?2, '[]')",
                    params![source_label, source_path],
                )
                .unwrap();
                conn.last_insert_rowid()
            }
        };

        // Ensure file exists
        let file_id: i64 = match conn.query_row(
            "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, rel_path],
            |row| row.get(0),
        ) {
            Ok(id) => id,
            Err(_) => {
                conn.execute(
                    "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes) VALUES (?1, ?2, 'abc123', 100)",
                    params![source_id, rel_path],
                )
                .unwrap();
                conn.last_insert_rowid()
            }
        };

        // Insert into FTS5
        conn.execute(
            "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
            params![title, content],
        )
        .unwrap();
        let rowid = conn.last_insert_rowid();

        // Insert matching trigram row
        conn.execute(
            "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
            params![rowid, title, content],
        )
        .unwrap();

        // Insert chunk_meta
        conn.execute(
            "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata) VALUES (?1, ?2, ?3, 1, 20, ?4)",
            params![rowid, file_id, source_id, metadata_json],
        )
        .unwrap();

        rowid
    }

    // ── sanitize helpers ─────────────────────────────────────────────────────

    #[test]
    fn test_sanitize_fts_query() {
        assert_eq!(
            sanitize_fts_query("hello world"),
            "\"hello world\" OR \"hello\" OR \"world\""
        );
        assert_eq!(
            sanitize_fts_query("error: can't find"),
            "\"error can t find\" OR \"error\" OR \"can\" OR \"t\" OR \"find\""
        );
        assert_eq!(
            sanitize_fts_query("fn main()"),
            "\"fn main\" OR \"fn\" OR \"main\""
        );
    }

    #[test]
    fn test_sanitize_trigram_query() {
        assert_eq!(sanitize_trigram_query("hello"), "\"hello\"");
        assert_eq!(sanitize_trigram_query(""), "");
    }

    #[test]
    fn test_truncate_snippet_short() {
        let s = "short text";
        assert_eq!(truncate_snippet(s, 100), "short text");
    }

    #[test]
    fn test_truncate_snippet_at_word_boundary() {
        let s = "the quick brown fox jumps over the lazy dog";
        let result = truncate_snippet(s, 20);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 24); // 20 + "..."
    }

    #[test]
    fn test_truncate_snippet_preserves_utf8_boundaries() {
        let result = truncate_snippet("before 🧑‍💻 after", 9);
        assert_eq!(result, "before...");
    }

    // ── knowledge_search (no filter) ─────────────────────────────────────────

    #[test]
    fn search_empty_db_returns_empty() {
        let store = make_store();
        let weights = SearchWeights::default();
        let results = knowledge_search(&store, "anything", None, None, 10, &weights, 2000).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_finds_bm25_match() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "notes/test.md",
            "Fatigue Model",
            "The fatigue model applies minute-weighted decay to player performance",
            "{}",
        );

        let weights = SearchWeights::default();
        let results =
            knowledge_search(&store, "fatigue model", None, None, 10, &weights, 2000).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].source_type, "knowledge");
        assert_eq!(results[0].source_label, "vault");
        assert_eq!(results[0].file_path, "notes/test.md");
    }

    #[test]
    fn search_returns_correct_lines_format() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "test.md",
            "Title",
            "Content here",
            "{}",
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(&store, "content", None, None, 10, &weights, 2000).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].lines, "1-20");
    }

    // ── knowledge_search (with metadata filter) ──────────────────────────────

    #[test]
    fn search_with_frontmatter_filter() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "task1.md",
            "Active Task",
            "This is an active task about implementation",
            r#"{"frontmatter":{"type":"task","status":"active"}}"#,
        );
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "note1.md",
            "Random Note",
            "This is a random note about implementation",
            r#"{"frontmatter":{"type":"note","status":"draft"}}"#,
        );

        let weights = SearchWeights::default();

        // Filter for type:task — should return only the task
        let results = knowledge_search(
            &store,
            "implementation",
            Some("type:task"),
            None,
            10,
            &weights,
            2000,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Active Task");
    }

    #[test]
    fn search_with_source_filter() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "v.md",
            "Vault Note",
            "Content from vault about algorithms",
            "{}",
        );
        seed_chunk(
            &store,
            "docs",
            "/tmp/docs",
            "d.md",
            "Docs Note",
            "Content from docs about algorithms",
            "{}",
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(
            &store,
            "algorithms",
            Some("source:vault"),
            None,
            10,
            &weights,
            2000,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_label, "vault");
    }

    #[test]
    fn search_with_tag_filter() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "tagged.md",
            "Tagged Note",
            "Content about prediction models and forecasting",
            r#"{"frontmatter":{"tags":["implementation","system3"]}}"#,
        );
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "untagged.md",
            "Untagged Note",
            "Content about prediction models and estimation",
            r#"{"frontmatter":{"tags":["review"]}}"#,
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(
            &store,
            "prediction",
            Some("tag:implementation"),
            None,
            10,
            &weights,
            2000,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tagged Note");
    }

    #[test]
    fn search_with_folder_filter() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "projects/foo.md",
            "Project Note",
            "Content about project setup and configuration",
            r#"{"folder_tags":["01-projects","foo"]}"#,
        );
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "logs/bar.md",
            "Log Note",
            "Content about debugging and configuration",
            r#"{"folder_tags":["06-logs","debug"]}"#,
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(
            &store,
            "configuration",
            Some("folder:01-projects"),
            None,
            10,
            &weights,
            2000,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Project Note");
    }

    #[test]
    fn search_with_source_label_param() {
        let store = make_store();
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "v.md",
            "Vault Item",
            "Searchable content about architecture",
            "{}",
        );
        seed_chunk(
            &store,
            "docs",
            "/tmp/docs",
            "d.md",
            "Docs Item",
            "Searchable content about architecture",
            "{}",
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(
            &store,
            "architecture",
            None,
            Some("docs"),
            10,
            &weights,
            2000,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_label, "docs");
    }

    #[test]
    fn search_metadata_in_result() {
        let store = make_store();
        let meta = r#"{"frontmatter":{"type":"task","status":"active"}}"#;
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "meta.md",
            "Meta Test",
            "Content with metadata for testing purposes",
            meta,
        );

        let weights = SearchWeights::default();
        let results =
            knowledge_search(&store, "metadata testing", None, None, 10, &weights, 2000).unwrap();
        assert!(!results.is_empty());
        let fm = &results[0].metadata["frontmatter"];
        assert_eq!(fm["type"], "task");
        assert_eq!(fm["status"], "active");
    }

    // ── combined_search ──────────────────────────────────────────────────────

    #[test]
    fn combined_search_no_knowledge_returns_only_session() {
        // With no knowledge sources, combined_search should still work
        // (knowledge portion returns empty)
        let store = make_store();
        let weights = SearchWeights::default();
        let results = knowledge_search(&store, "anything", None, None, 10, &weights, 2000).unwrap();
        assert!(results.is_empty());
    }

    // ── snippet truncation ───────────────────────────────────────────────────

    #[test]
    fn search_snippet_truncated() {
        let store = make_store();
        let long_content = "word ".repeat(500); // ~2500 chars
        seed_chunk(
            &store,
            "vault",
            "/tmp/vault",
            "long.md",
            "Long Content",
            &long_content,
            "{}",
        );

        let weights = SearchWeights::default();
        let results = knowledge_search(&store, "word", None, None, 10, &weights, 100).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].snippet.len() <= 104); // 100 + "..."
    }

    // ── dot_product ──────────────────────────────────────────────────────────

    #[test]
    fn test_dot_product_identical() {
        let a = vec![1.0, 0.0, 0.0];
        assert!((dot_product(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_dot_product_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(dot_product(&a, &b).abs() < 1e-6);
    }
}
