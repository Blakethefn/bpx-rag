pub mod profile;
pub mod truncate;

use anyhow::Result;

use crate::config::Config;
use crate::knowledge::filter::parse_filter;
use crate::knowledge::query::{knowledge_search_report, KnowledgeSearchResult, SearchWeights};
use crate::knowledge::KnowledgeStore;
use crate::search::profile::{SearchProfile, SearchWhy};

#[derive(Debug, Clone)]
pub struct QueryRequest<'a> {
    pub query: &'a str,
    pub source: Option<&'a str>,
    pub knowledge_filter: Option<&'a str>,
    pub limit: u32,
    pub snippet_bytes: usize,
    pub profile: SearchProfile,
    pub explain: bool,
}

/// Run a hybrid knowledge search and render the result text.
///
/// With `count_only`, returns a JSON summary (result count, byte estimate,
/// matched sources) instead of result snippets.
pub fn run_query(
    request: QueryRequest<'_>,
    config: &Config,
    knowledge: &KnowledgeStore,
    count_only: bool,
) -> Result<String> {
    let explicit_profile = request.profile.is_explicit();
    let effective_query = if explicit_profile {
        request.profile.effective_query(request.query)
    } else {
        request.query.trim().to_string()
    };
    let weights = if explicit_profile {
        SearchWeights {
            keyword_weight: config.search.keyword_weight
                * request.profile.keyword_weight_multiplier(),
            vector_weight: config.search.vector_weight * request.profile.vector_weight_multiplier(),
        }
    } else {
        SearchWeights {
            keyword_weight: config.search.keyword_weight,
            vector_weight: config.search.vector_weight,
        }
    };

    let scoped = request.source.is_some() || request.knowledge_filter.is_some();
    let report = knowledge_search_report(
        knowledge,
        &effective_query,
        request.knowledge_filter,
        request.source,
        request.limit,
        &weights,
        request.snippet_bytes,
    )?;
    let warnings = report.warnings;
    let mut results = report.results;
    apply_knowledge_bias(&mut results, request.profile);
    apply_graph_rerank(
        knowledge,
        &mut results,
        request.profile,
        scoped,
        request.knowledge_filter,
        request.source,
    )?;
    if request.knowledge_filter.is_some() {
        for result in &mut results {
            result.why.metadata_match = true;
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(request.limit as usize);

    if count_only {
        return count_summary(request.query, request.profile, &results, &warnings);
    }

    let top_snippet_bytes = request
        .profile
        .top_snippet_override()
        .unwrap_or(request.snippet_bytes);
    let secondary_snippet_bytes = request
        .profile
        .secondary_snippet_override()
        .unwrap_or(config.search.secondary_snippet_bytes);
    Ok(format_results(
        request.query,
        request.explain,
        &warnings,
        &results,
        top_snippet_bytes,
        secondary_snippet_bytes,
        config.search.top_result_count,
    ))
}

fn count_summary(
    query: &str,
    profile: SearchProfile,
    results: &[KnowledgeSearchResult],
    warnings: &[String],
) -> Result<String> {
    let estimated_bytes: u64 = results.iter().map(|r| r.snippet.len() as u64).sum();
    let estimated_tokens = (estimated_bytes as f64 / 4.0).ceil() as u32;
    let recommendation = if estimated_tokens <= 20_000 {
        "inline"
    } else {
        "delegate"
    };
    let mut sources_matched: Vec<&str> = results.iter().map(|r| r.source_label.as_str()).collect();
    sources_matched.sort_unstable();
    sources_matched.dedup();
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "count_only": true,
        "query": query,
        "profile": profile.as_str(),
        "total_chunks": results.len(),
        "warnings": warnings,
        "estimated_bytes": estimated_bytes,
        "estimated_tokens": estimated_tokens,
        "sources_matched": sources_matched,
        "recommendation": recommendation
    }))?)
}

fn apply_knowledge_bias(results: &mut [KnowledgeSearchResult], profile: SearchProfile) {
    if matches!(profile, SearchProfile::Default) {
        return;
    }
    let multiplier = profile.knowledge_bias_multiplier();
    for result in results {
        let boosted = result.score * multiplier;
        result.why.layers.knowledge_bias += boosted - result.score;
        result.score = boosted;
    }
}

fn apply_graph_rerank(
    ks: &KnowledgeStore,
    results: &mut Vec<KnowledgeSearchResult>,
    profile: SearchProfile,
    scoped_filtering: bool,
    filter_hint: Option<&str>,
    source_scope: Option<&str>,
) -> Result<()> {
    let boost = profile.graph_boost(scoped_filtering);
    if boost <= 0.0 || results.is_empty() {
        return Ok(());
    }

    let predicates = parse_filter(filter_hint)?;
    let preds = if predicates.is_empty() {
        None
    } else {
        Some(predicates.as_slice())
    };

    let seed_ids: Vec<i64> = results.iter().take(3).map(|r| r.file_id).collect();
    let mut linked_chunks = Vec::new();
    for file_id in seed_ids {
        linked_chunks.extend(ks.get_related_chunks(file_id, 6, preds)?);
    }

    let mut neighbor_by_file: std::collections::HashMap<i64, crate::knowledge::RelatedChunk> =
        std::collections::HashMap::new();
    let mut source_by_file: std::collections::HashMap<i64, (String, i64)> =
        std::collections::HashMap::new();
    for chunk in linked_chunks {
        if let Some(scope) = source_scope {
            let source = knowledge_source_for_file(ks, chunk.file_id)?;
            if scope != source.0 {
                continue;
            }
            source_by_file.insert(chunk.file_id, source);
        }
        let chunk_has_lines = chunk.line_start > 0 && chunk.line_end >= chunk.line_start;
        match neighbor_by_file.entry(chunk.file_id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(chunk);
            }
            std::collections::hash_map::Entry::Occupied(mut entry)
                if (entry.get().line_start == 0
                    || entry.get().line_end < entry.get().line_start)
                    && chunk_has_lines =>
            {
                entry.insert(chunk);
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }
    }

    let existing_files: std::collections::HashSet<i64> =
        results.iter().map(|r| r.file_id).collect();
    for result in results.iter_mut() {
        if neighbor_by_file.contains_key(&result.file_id) {
            result.score += boost;
            result.why.layers.graph_boost += boost;
            result.why.graph_neighbor = true;
        }
    }

    if profile.inject_neighbors() && !neighbor_by_file.is_empty() {
        let baseline = results.last().map(|r| r.score * 0.92).unwrap_or(boost);
        let mut injected = 0usize;
        for (file_id, chunk) in neighbor_by_file {
            if injected >= 2 || existing_files.contains(&file_id) {
                continue;
            }
            if chunk.line_start == 0 || chunk.line_end < chunk.line_start {
                continue;
            }
            let (source_label, source_id) = match source_by_file.get(&file_id) {
                Some((label, id)) => (label.clone(), *id),
                None => knowledge_source_for_file(ks, file_id)?,
            };
            let mut why = SearchWhy::default();
            why.layers.graph_boost = boost;
            why.graph_neighbor = true;
            results.push(KnowledgeSearchResult {
                title: chunk.title,
                snippet: chunk.content,
                score: baseline + boost,
                source_label,
                file_path: chunk.rel_path,
                file_id,
                source_id,
                lines: format!("{}-{}", chunk.line_start, chunk.line_end),
                metadata: serde_json::Value::Object(Default::default()),
                source_type: "knowledge".to_string(),
                why,
            });
            injected += 1;
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(())
}

fn knowledge_source_for_file(ks: &KnowledgeStore, file_id: i64) -> Result<(String, i64)> {
    Ok(ks.conn().query_row(
        "SELECT ks.label, ks.id
         FROM knowledge_files kf
         JOIN knowledge_sources ks ON ks.id = kf.source_id
         WHERE kf.id = ?1",
        rusqlite::params![file_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

fn format_results(
    original_query: &str,
    explain: bool,
    warnings: &[String],
    results: &[KnowledgeSearchResult],
    top_snippet_bytes: usize,
    secondary_snippet_bytes: usize,
    top_result_count: usize,
) -> String {
    let mut output = format!("### \"{original_query}\"\n");
    for warning in warnings {
        output.push_str(&format!("Warning: {warning}\n"));
    }
    if results.is_empty() {
        output.push_str("No results.\n");
        return output;
    }

    let mut truncated_any = false;
    for (idx, result) in results.iter().enumerate() {
        let max_bytes = if idx < top_result_count {
            top_snippet_bytes
        } else {
            secondary_snippet_bytes
        };
        let snippet = crate::search::truncate::preview(&result.snippet, max_bytes);
        if result.snippet.len() > max_bytes {
            truncated_any = true;
        }
        output.push_str(&format!(
            "**[{}]** (source: {}, type: knowledge, file: {}, score: {:.4} (lines {}))\n{}\n{}\n\n",
            result.title,
            result.source_label,
            result.file_path,
            result.score,
            result.lines,
            result.why.compact_line(),
            snippet
        ));
        if explain {
            output.push_str(&format!("{}\n\n", result.why.detailed_line(result.score)));
        }
    }

    if truncated_any {
        output.push_str("Some results were truncated. Use bpx_read with the source label, cited relative path, and line range to read the matching full chunk.\n");
    }
    output
}
