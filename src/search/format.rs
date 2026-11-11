use anyhow::Result;

use crate::knowledge::freshness::{Freshness, FreshnessState};
use crate::knowledge::query::{KnowledgeSearchResult, LayerOutcome, LayerReport};
use crate::search::profile::SearchProfile;

/// How much snippet text each result may show, and how many count as top results.
#[derive(Debug, Clone, Copy)]
pub(super) struct SnippetBudget {
    pub(super) top_bytes: usize,
    pub(super) secondary_bytes: usize,
    pub(super) top_result_count: usize,
}

pub(super) fn count_summary(
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

/// One line that says what each layer did. Intentional skips appear here and nowhere else.
pub(super) fn layer_summary(layers: &[LayerReport]) -> String {
    let parts: Vec<String> = layers
        .iter()
        .map(|layer| match &layer.outcome {
            LayerOutcome::Ran => format!("{} ran ({} candidates)", layer.layer, layer.candidates),
            LayerOutcome::Skipped { reason } => {
                format!("{} skipped ({})", layer.layer, reason.as_str())
            }
            LayerOutcome::Failed { error } => format!("{} failed ({error})", layer.layer),
        })
        .collect();
    format!("layers: {}\n", parts.join("; "))
}

pub(super) fn format_results(
    original_query: &str,
    explain: bool,
    warnings: &[String],
    layers: &[LayerReport],
    results: &[KnowledgeSearchResult],
    related: &[Vec<String>],
    freshness: &[Freshness],
    budget: SnippetBudget,
) -> String {
    let mut output = format!("### \"{original_query}\"\n");
    for warning in warnings {
        output.push_str(&format!("Warning: {warning}\n"));
    }
    if explain {
        output.push_str(&layer_summary(layers));
    }
    if results.is_empty() {
        output.push_str("No results.\n");
        return output;
    }

    let mut truncated_any = false;
    for (idx, result) in results.iter().enumerate() {
        let max_bytes = if idx < budget.top_result_count {
            budget.top_bytes
        } else {
            budget.secondary_bytes
        };
        let snippet = crate::search::truncate::preview(&result.snippet, max_bytes);
        if result.snippet.len() > max_bytes {
            truncated_any = true;
        }
        let related_line = related
            .get(idx)
            .filter(|files| !files.is_empty())
            .map(|files| format!("related: {}\n", files.join(", ")))
            .unwrap_or_default();
        let freshness_label = freshness
            .get(idx)
            .map_or_else(|| "unknown (not checked)".to_string(), Freshness::label);
        output.push_str(&format!(
            "**[{}]** (source: {}, type: knowledge, file: {}, score: {:.4} (lines {}), freshness: {})\n{}\n{}{}\n\n",
            result.title,
            result.source_label,
            result.file_path,
            result.score,
            result.lines,
            freshness_label,
            result.why.compact_line(),
            related_line,
            snippet
        ));
        if explain {
            output.push_str(&format!("{}\n\n", result.why.detailed_line(result.score)));
        }
    }

    if truncated_any {
        output.push_str("Some results were truncated. Use bpx_read with the source label, cited relative path, and line range to read the matching full chunk.\n");
    }
    if freshness
        .iter()
        .any(|check| check.state == FreshnessState::Stale)
    {
        output.push_str("Some results are stale: their files changed after they were indexed. Run sync for the source, or enable watch mode.\n");
    }
    output
}

