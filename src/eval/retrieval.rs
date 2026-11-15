use super::*;
use crate::config::Config;
use crate::knowledge;
use crate::knowledge::query::{LayerPolicy, SearchWeights};
use anyhow::{anyhow, Result};
use load::index::index_corpus;
use load::model::load_model;
use load::{load_eval_data, resolve_eval_paths, validate_run_options};
use record::{evaluate_query, verify_inputs_unchanged, RetrievalTally};
use search::RunContext;
use std::path::Path;

mod dataset;
mod load;
mod metrics;
mod record;
mod report;
mod search;
pub(super) use dataset::*;
pub(super) use metrics::*;
pub use report::*;

pub fn run_retrieval_evaluation(
    repetitions: usize,
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
    hybrid_weights: SearchWeights,
    layer_policy: LayerPolicy,
) -> Result<RetrievalEvalReport> {
    validate_run_options(repetitions, &hybrid_weights)?;
    let config = Config::load()?;
    let model = load_model(&config)?;
    let paths = resolve_eval_paths(dataset, corpus_root)?;
    let data = load_eval_data(&paths)?;
    let index = index_corpus(model.embedder, &paths.corpus_dir)?;

    let run = RunContext {
        store: &index.store,
        corpus_dir: &paths.corpus_dir,
        hybrid_weights: &hybrid_weights,
        layer_policy,
        repetitions,
    };
    let mut tally = RetrievalTally::with_capacity(data.queries.len(), repetitions);
    for query in &data.queries {
        let judgments = data
            .qrels
            .get(&query.query_id)
            .ok_or_else(|| anyhow!("no relevance judgments for query {}", query.query_id))?;
        evaluate_query(&run, query, judgments, &mut tally)?;
    }

    verify_inputs_unchanged(&paths, &data)?;
    tally.latencies.sort_by(duration_cmp);
    let latency_ms = tally
        .latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    let db_bytes = in_memory_database_bytes(index.store.conn())?;

    Ok(RetrievalEvalReport {
        model_name: config.embeddings.model,
        model_id: model.model_id,
        embedding_device: config.embeddings.device.as_str().to_string(),
        layer_policy,
        dataset_name: paths
            .data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("dataset")
            .to_string(),
        corpus_root_name: paths
            .corpus_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("corpus")
            .to_string(),
        embedding_dimension: model.embedding_dimension,
        batch_size: config.embeddings.batch_size,
        intra_threads: config.embeddings.intra_threads,
        candidate_oversample_factor: knowledge::query::CANDIDATE_OVERSAMPLE_FACTOR,
        hybrid_keyword_weight: hybrid_weights.keyword_weight,
        hybrid_vector_weight: hybrid_weights.vector_weight,
        queries_sha256: data.queries_sha256,
        qrels_sha256: data.qrels_sha256,
        corpus_manifest_sha256: data.corpus_manifest_sha256,
        positive_judgment_count: data.positive_judgment_count,
        explicit_negative_judgment_count: data.explicit_negative_judgment_count,
        judgment_sources: data.judgment_sources,
        operating_system: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        host_threads: std::thread::available_parallelism().map_or(1, usize::from),
        query_count: data.queries.len(),
        answerable_query_count: tally.overall.count,
        unanswerable_query_count: tally.unanswerable_hybrid.count,
        corpus_file_count: index.sync.files_scanned,
        sensitive_files_skipped: index.sync.sensitive_files_skipped,
        chunk_count: index.sync.chunks_total,
        tree_sitter_files_indexed: index.sync.tree_sitter_files_indexed,
        existing_chunker_files_indexed: index.sync.existing_chunker_files_indexed,
        tree_sitter_chunks_indexed: index.sync.tree_sitter_chunks_indexed,
        existing_chunker_chunks_indexed: index.sync.existing_chunker_chunks_indexed,
        repetitions,
        model_load_ms: model.model_load_ms,
        indexing_ms: index.indexing_ms,
        database_bytes: db_bytes,
        model_artifact_bytes: model.model_artifact_bytes,
        embedding_ms: index.sync.embedding_ms,
        embedding_calls: index.sync.embedding_calls,
        embedding_chunks: index.sync.embedding_chunks,
        sqlite_transaction_ms: index.sync.transaction_ms,
        warm_query_p50_ms: percentile(&latency_ms, 0.50),
        warm_query_p95_ms: percentile(&latency_ms, 0.95),
        overall: tally.overall.average(),
        with_relation_neighbors: tally.with_relations.average(),
        queries_with_relation_neighbors: tally.queries_with_neighbors,
        lexical_only: tally.lexical_only.average(),
        vector_only: tally.vector_only.average(),
        unanswerable_hybrid: tally.unanswerable_hybrid.average(),
        unanswerable_lexical_only: tally.unanswerable_lexical.average(),
        unanswerable_vector_only: tally.unanswerable_vector.average(),
        cases: tally.cases,
        by_category: tally
            .categories
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        by_language: tally
            .languages
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        citation_content_validity: tally.citations.content_validity(),
        citation_content_matches: tally.citations.content_matches,
        citation_validity: tally.citations.validity(),
        citation_count: tally.citations.checked,
        citation_content_mismatches: tally.citations.content_mismatches,
        invalid_citations: tally.citations.invalid,
        filter_leaks: tally.filter_leaks,
    })
}
