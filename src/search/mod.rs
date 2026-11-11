pub mod profile;
pub mod truncate;

mod format;
mod rerank;

use format::{count_summary, format_results, SnippetBudget};
#[cfg(test)]
use format::layer_summary;
use rerank::{apply_graph_rerank, apply_knowledge_bias};

pub(crate) use rerank::attach_relation_neighbors;

/// Linked files listed under each top result.
const RELATED_FILES_PER_RESULT: usize = 5;

use anyhow::Result;

use crate::config::Config;
use crate::knowledge::freshness::check_results;
use crate::knowledge::query::{
    knowledge_search_report_with_policy, KnowledgeSearchResult, LayerPolicy, SearchScope,
    SearchWeights,
};
#[cfg(test)]
use crate::knowledge::query::{LayerOutcome, LayerReport};
use crate::knowledge::KnowledgeStore;
use crate::search::profile::SearchProfile;

#[derive(Debug, Clone)]
pub struct QueryRequest<'a> {
    pub query: &'a str,
    pub source: Option<&'a str>,
    pub knowledge_filter: Option<&'a str>,
    pub limit: u32,
    /// Most passages kept from one file before other files fill the limit. Must be at least 1.
    pub max_per_file: u32,
    pub snippet_bytes: usize,
    pub profile: SearchProfile,
    pub explain: bool,
    /// Why vector retrieval is not running for this search (for example, the model could not
    /// load, or CUDA was configured on a CPU-only build). `--explain` shows it first, as a
    /// warning, because ranking then uses the keyword layers only. `None` when vectors run.
    pub vector_unavailable: Option<&'a str>,
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
    let report = knowledge_search_report_with_policy(
        knowledge,
        &effective_query,
        SearchScope {
            filter: request.knowledge_filter,
            source_label: request.source,
            policy: LayerPolicy::default(),
            max_per_file: request.max_per_file,
        },
        request.limit,
        &weights,
        request.snippet_bytes,
    )?;
    let mut warnings = report.warnings;
    if let (true, Some(reason)) = (request.explain, request.vector_unavailable) {
        warnings.insert(
            0,
            format!("vector retrieval is not running, so ranking uses the keyword layers only: {reason}"),
        );
    }
    let layers = report.layers;
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
    if !explicit_profile && config.search.relation_neighbors > 0 {
        results = attach_relation_neighbors(
            knowledge,
            results,
            config.search.relation_neighbors,
            request.limit as usize,
        )?;
    }

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
    let related = related_files_for(knowledge, &results, config.search.top_result_count)?;
    let freshness = check_results(knowledge, &results)?;
    Ok(format_results(
        request.query,
        request.explain,
        &warnings,
        &layers,
        &results,
        &related,
        &freshness,
        SnippetBudget {
            top_bytes: top_snippet_bytes,
            secondary_bytes: secondary_snippet_bytes,
            top_result_count: config.search.top_result_count,
        },
    ))
}

/// Linked files to list under each top result, aligned by index with `results`.
fn related_files_for(
    knowledge: &KnowledgeStore,
    results: &[KnowledgeSearchResult],
    top_result_count: usize,
) -> Result<Vec<Vec<String>>> {
    results
        .iter()
        .take(top_result_count)
        .map(|result| knowledge.related_files(result.file_id, RELATED_FILES_PER_RESULT))
        .collect()
}

#[cfg(test)]
mod tests;
