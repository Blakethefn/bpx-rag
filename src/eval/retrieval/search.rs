use super::super::pool::PoolChannel;
use super::*;
use crate::knowledge::query::{
    KnowledgeSearchResult, LayerPolicy, SearchReport, SearchScope, SearchWeights,
};
use crate::knowledge::KnowledgeStore;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) struct RunContext<'a> {
    pub(super) store: &'a KnowledgeStore,
    pub(super) corpus_dir: &'a Path,
    pub(super) hybrid_weights: &'a SearchWeights,
    pub(super) layer_policy: LayerPolicy,
    pub(super) repetitions: usize,
}

pub(super) struct QuerySearches {
    pub(super) layers: Vec<crate::knowledge::query::LayerReport>,
    pub(super) results: Vec<KnowledgeSearchResult>,
    pub(super) lexical_results: Vec<KnowledgeSearchResult>,
    pub(super) vector_results: Vec<KnowledgeSearchResult>,
    pub(super) query_times: Vec<Duration>,
}

pub(super) struct QueryScores {
    pub(super) hybrid_paths: Vec<String>,
    pub(super) lexical_paths: Vec<String>,
    pub(super) vector_paths: Vec<String>,
    pub(super) hybrid: QualityMetrics,
    pub(super) lexical: QualityMetrics,
    pub(super) vector: QualityMetrics,
}

pub(super) fn query_scope(query: &EvalQuery, layer_policy: LayerPolicy) -> SearchScope<'_> {
    SearchScope {
        filter: query.filter.as_deref(),
        source_label: Some("retrieval-eval"),
        policy: layer_policy,
        max_per_file: 1,
    }
}

fn search_report(
    store: &KnowledgeStore,
    text: &str,
    scope: SearchScope<'_>,
    weights: &SearchWeights,
) -> Result<SearchReport> {
    knowledge::query::knowledge_search_report_with_policy(
        store,
        text,
        scope,
        TOP_K as u32,
        weights,
        1024,
    )
}

pub(super) fn run_searches(
    run: &RunContext<'_>,
    query: &EvalQuery,
    scope: SearchScope<'_>,
) -> Result<QuerySearches> {
    let _ = search_report(run.store, &query.text, scope, run.hybrid_weights)?;
    let lexical_results = search_report(
        run.store,
        &query.text,
        scope,
        &PoolChannel::Lexical.weights(),
    )?
    .results;
    let vector_results = search_report(
        run.store,
        &query.text,
        scope,
        &PoolChannel::Vector.weights(),
    )?
    .results;
    let mut hybrid = SearchReport::default();
    let mut query_times = Vec::with_capacity(run.repetitions);
    for _ in 0..run.repetitions {
        let started = Instant::now();
        hybrid = search_report(run.store, &query.text, scope, run.hybrid_weights)?;
        query_times.push(started.elapsed());
    }
    Ok(QuerySearches {
        layers: hybrid.layers,
        results: hybrid.results,
        lexical_results,
        vector_results,
        query_times,
    })
}

pub(super) fn file_paths(results: &[KnowledgeSearchResult]) -> Vec<String> {
    results
        .iter()
        .map(|result| result.file_path.clone())
        .collect()
}

pub(super) fn score_query(
    results: &[KnowledgeSearchResult],
    lexical_results: &[KnowledgeSearchResult],
    vector_results: &[KnowledgeSearchResult],
    judgments: &BTreeMap<String, u8>,
) -> QueryScores {
    let hybrid_paths = file_paths(results);
    let lexical_paths = file_paths(lexical_results);
    let vector_paths = file_paths(vector_results);
    QueryScores {
        hybrid: measure_query(&hybrid_paths, judgments),
        lexical: measure_query(&lexical_paths, judgments),
        vector: measure_query(&vector_paths, judgments),
        hybrid_paths,
        lexical_paths,
        vector_paths,
    }
}

pub(super) fn search_deep_paths(
    run: &RunContext<'_>,
    query: &EvalQuery,
    scope: SearchScope<'_>,
) -> Result<Vec<String>> {
    let deep_results = knowledge::query::search_with_pool(
        run.store,
        &query.text,
        scope,
        DIAGNOSTIC_DEPTH as u32,
        knowledge::query::expanded_candidate_limit(TOP_K as u32),
        run.hybrid_weights,
        1024,
    )?
    .results;
    Ok(unique_paths(&file_paths(&deep_results), DIAGNOSTIC_DEPTH))
}
