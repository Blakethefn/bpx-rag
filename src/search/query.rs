use anyhow::Result;

use crate::config::Config;
use crate::knowledge::freshness::check_results;
use crate::knowledge::query::{
    knowledge_search_report_with_policy, KnowledgeSearchResult, LayerPolicy, SearchScope,
    SearchWeights,
};
use crate::knowledge::KnowledgeStore;

use super::format::{count_summary, format_results, FormatInput, SnippetBudget};
use super::rerank::{apply_graph_rerank, apply_knowledge_bias};
use super::{attach_relation_neighbors, QueryRequest};

const RELATED_FILES_PER_RESULT: usize = 5;

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
    if !explicit_profile && !scoped && config.search.relation_neighbors > 0 {
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
    let related = if scoped {
        Vec::new()
    } else {
        related_files_for(knowledge, &results, config.search.top_result_count)?
    };
    let freshness = check_results(knowledge, &results)?;
    Ok(format_results(FormatInput {
        original_query: request.query,
        explain: request.explain,
        warnings: &warnings,
        layers: &layers,
        results: &results,
        related: &related,
        freshness: &freshness,
        budget: SnippetBudget {
            top_bytes: top_snippet_bytes,
            secondary_bytes: secondary_snippet_bytes,
            top_result_count: config.search.top_result_count,
        },
    }))
}

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
