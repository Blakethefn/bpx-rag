use super::*;
use crate::search::profile::SearchWhy;
use anyhow::Context;
use std::collections::HashMap;

/// Keep up to `max_per_file` passages from each file, taken in score order,
/// until `limit` passages are selected. Passages past that cap are used only
/// to fill slots that the capped passages cannot, again in score order, so a
/// corpus with few files still returns `limit` results. With
/// `max_per_file = 1` this is one passage per file first, then repeats in
/// score order, and the output order is file-diversity-first. The caller
/// must reject `max_per_file = 0`.
pub(super) fn diversify_by_file(
    results: Vec<KnowledgeSearchResult>,
    limit: usize,
    max_per_file: usize,
) -> Vec<KnowledgeSearchResult> {
    if limit == 0 {
        return Vec::new();
    }
    let mut kept_per_file: HashMap<i64, usize> = HashMap::new();
    let mut selected = Vec::with_capacity(limit);
    let mut overflow = Vec::new();
    for result in results {
        let kept = kept_per_file.entry(result.file_id).or_insert(0);
        if *kept < max_per_file {
            *kept += 1;
            selected.push(result);
            if selected.len() == limit {
                return selected;
            }
        } else {
            overflow.push(result);
        }
    }
    selected.extend(overflow.into_iter().take(limit - selected.len()));
    selected
}

/// A chunk row returned by one retrieval layer, before RRF fusion.
#[derive(Debug)]
pub(super) struct RankedResult {
    pub(super) rowid: i64,
    pub(super) file_id: i64,
    pub(super) source_id: i64,
    pub(super) title: String,
    pub(super) content: String,
    pub(super) rank: f64,
    pub(super) source_label: String,
    pub(super) file_path: String,
    pub(super) line_start: u32,
    pub(super) line_end: u32,
    pub(super) metadata: String,
}

pub(super) struct ScoredResult {
    pub(super) file_id: i64,
    pub(super) source_id: i64,
    pub(super) title: String,
    pub(super) content: String,
    pub(super) source_label: String,
    pub(super) file_path: String,
    pub(super) line_start: u32,
    pub(super) line_end: u32,
    pub(super) metadata: String,
    pub(super) rrf_score: f64,
    pub(super) why: SearchWhy,
}

impl ScoredResult {
    /// Build the output row. Chunk metadata is written by sync as JSON, so
    /// unparseable metadata is reported rather than shown as empty metadata.
    pub(super) fn into_knowledge_result(
        self,
        snippet_bytes: usize,
    ) -> anyhow::Result<KnowledgeSearchResult> {
        let metadata: serde_json::Value = serde_json::from_str(&self.metadata)
            .with_context(|| format!("invalid chunk metadata for {}", self.file_path))?;
        Ok(KnowledgeSearchResult {
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
        })
    }
}

/// RRF merge with K=60.
pub(super) fn merge_results(
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
                    "identifier" => sr.why.layers.identifier += rrf_score,
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
                        "identifier" => why.layers.identifier = rrf_score,
                        "vector" => why.layers.vector = rrf_score,
                        _ => {}
                    }
                    why
                },
            });
    }
}

/// Truncate content to a snippet at word boundaries.
pub(super) fn truncate_snippet(content: &str, max_bytes: usize) -> String {
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
