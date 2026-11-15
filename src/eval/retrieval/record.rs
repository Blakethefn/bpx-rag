use super::load::{EvalData, EvalPaths};
use super::search::{
    query_scope, run_searches, score_query, search_deep_paths, QuerySearches, RunContext,
};
use super::*;
use anyhow::Result;
use std::collections::BTreeMap;
use std::time::Duration;

mod case;
mod totals;
use case::{build_case, count_filter_leaks, record_citations};
use totals::{record_query_totals, record_relation_neighbors};

#[derive(Default)]
pub(super) struct CitationTally {
    pub(super) checked: usize,
    valid: usize,
    pub(super) invalid: Vec<CitationIssue>,
    pub(super) content_matches: usize,
    pub(super) content_mismatches: Vec<CitationIssue>,
}

impl CitationTally {
    pub(super) fn content_validity(&self) -> Option<f64> {
        (self.checked > 0).then_some(self.content_matches as f64 / self.checked as f64)
    }

    pub(super) fn validity(&self) -> f64 {
        if self.checked == 0 {
            1.0
        } else {
            self.valid as f64 / self.checked as f64
        }
    }
}

#[derive(Default)]
pub(super) struct RetrievalTally {
    pub(super) overall: MetricSums,
    pub(super) with_relations: MetricSums,
    pub(super) queries_with_neighbors: usize,
    pub(super) lexical_only: MetricSums,
    pub(super) vector_only: MetricSums,
    pub(super) categories: BTreeMap<String, MetricSums>,
    pub(super) languages: BTreeMap<String, MetricSums>,
    pub(super) unanswerable_hybrid: UnanswerableMetricSums,
    pub(super) unanswerable_lexical: UnanswerableMetricSums,
    pub(super) unanswerable_vector: UnanswerableMetricSums,
    pub(super) cases: Vec<EvalCaseReport>,
    pub(super) latencies: Vec<Duration>,
    pub(super) citations: CitationTally,
    pub(super) filter_leaks: usize,
}

impl RetrievalTally {
    pub(super) fn with_capacity(query_count: usize, repetitions: usize) -> Self {
        Self {
            cases: Vec::with_capacity(query_count),
            latencies: Vec::with_capacity(query_count * repetitions),
            ..Self::default()
        }
    }
}

pub(super) fn evaluate_query(
    run: &RunContext<'_>,
    query: &EvalQuery,
    judgments: &BTreeMap<String, u8>,
    tally: &mut RetrievalTally,
) -> Result<()> {
    let scope = query_scope(query, run.layer_policy);
    let QuerySearches {
        layers,
        results,
        lexical_results,
        vector_results,
        query_times,
    } = run_searches(run, query, scope)?;
    tally.latencies.extend(query_times);

    let scores = score_query(&results, &lexical_results, &vector_results, judgments);
    record_relation_neighbors(run.store, &results, judgments, tally)?;
    let unanswerable_hybrid = record_query_totals(tally, query, &scores, judgments);
    let deep_paths = search_deep_paths(run, query, scope)?;
    tally.cases.push(build_case(
        query,
        scores,
        unanswerable_hybrid,
        deep_paths,
        layers,
    ));
    tally.filter_leaks += count_filter_leaks(&results, query.expected_language.as_deref());
    record_citations(
        run.corpus_dir,
        &query.query_id,
        &results,
        &mut tally.citations,
    )?;
    Ok(())
}

pub(super) fn verify_inputs_unchanged(paths: &EvalPaths, data: &EvalData) -> Result<()> {
    if sha256_file(&paths.query_path)? != data.queries_sha256
        || sha256_file(&paths.qrels_path)? != data.qrels_sha256
    {
        anyhow::bail!("retrieval evaluation dataset changed while it was running; retry");
    }
    if corpus_manifest_sha256(&paths.corpus_dir)? != data.corpus_manifest_sha256 {
        anyhow::bail!("retrieval corpus changed while evaluation was running; retry");
    }
    Ok(())
}
