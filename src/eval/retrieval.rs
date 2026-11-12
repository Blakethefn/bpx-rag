use super::pool::PoolChannel;
use super::*;
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use crate::knowledge::query::{
    KnowledgeSearchResult, LayerPolicy, LayerReport, SearchReport, SearchScope, SearchWeights,
};
use crate::knowledge::sync::SyncResult;
use crate::knowledge::KnowledgeStore;
use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod dataset;
mod metrics;
mod report;
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

fn validate_run_options(repetitions: usize, hybrid_weights: &SearchWeights) -> Result<()> {
    if !(1..=100).contains(&repetitions) {
        return Err(anyhow!("repetitions must be between 1 and 100"));
    }
    if !hybrid_weights.keyword_weight.is_finite()
        || !hybrid_weights.vector_weight.is_finite()
        || !(0.0..=10.0).contains(&hybrid_weights.keyword_weight)
        || !(0.0..=10.0).contains(&hybrid_weights.vector_weight)
        || hybrid_weights.keyword_weight == 0.0
        || hybrid_weights.vector_weight == 0.0
    {
        anyhow::bail!("hybrid weights must be finite and in the range (0, 10]");
    }
    Ok(())
}

/// Embedder and its cache facts, loaded from the configured model directory.
struct LoadedModel {
    embedder: Embedder,
    model_id: String,
    embedding_dimension: usize,
    model_artifact_bytes: u64,
    model_load_ms: f64,
}

fn load_model(config: &Config) -> Result<LoadedModel> {
    let model_dir = crate::app::model_directory(config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        return Err(anyhow!(
            "evaluation requires the configured model to be cached; run `bpx-rag models download` first ({}).",
            model_dir.display()
        ));
    }

    let model_start = Instant::now();
    let embedder = Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
    )?;
    let model_load_ms = model_start.elapsed().as_secs_f64() * 1000.0;
    let model_id = embedder.model_id().to_string();
    let embedding_dimension = embedder.dim();
    let model_artifact_bytes = embed::model_cache_bytes(&model_dir, &config.embeddings.model)?;
    Ok(LoadedModel {
        embedder,
        model_id,
        embedding_dimension,
        model_artifact_bytes,
        model_load_ms,
    })
}

/// Dataset and corpus locations, with the dataset directories checked to exist.
struct EvalPaths {
    data_dir: PathBuf,
    corpus_dir: PathBuf,
    query_path: PathBuf,
    qrels_path: PathBuf,
}

fn resolve_eval_paths(dataset: Option<&Path>, corpus_root: Option<&Path>) -> Result<EvalPaths> {
    let data_dir = dataset.map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/retrieval"),
        Path::to_path_buf,
    );
    let corpus_dir = corpus_root.map_or_else(|| data_dir.join("corpus"), Path::to_path_buf);
    if !data_dir.is_dir() {
        anyhow::bail!(
            "retrieval dataset directory does not exist: {}",
            data_dir.display()
        );
    }
    if !corpus_dir.is_dir() {
        anyhow::bail!(
            "retrieval corpus directory does not exist: {}",
            corpus_dir.display()
        );
    }
    let query_path = data_dir.join("queries.jsonl");
    let qrels_path = data_dir.join("qrels.jsonl");
    Ok(EvalPaths {
        data_dir,
        corpus_dir,
        query_path,
        qrels_path,
    })
}

/// Validated judgments plus the fingerprints and counts reported with them.
struct EvalData {
    queries: Vec<EvalQuery>,
    qrels: HashMap<String, BTreeMap<String, u8>>,
    queries_sha256: String,
    qrels_sha256: String,
    corpus_manifest_sha256: String,
    positive_judgment_count: usize,
    explicit_negative_judgment_count: usize,
    judgment_sources: Vec<String>,
}

fn load_eval_data(paths: &EvalPaths) -> Result<EvalData> {
    let queries = read_jsonl::<EvalQuery>(&paths.query_path)?;
    let qrels = read_qrels(&paths.qrels_path)?;
    validate_qrels(&queries, &qrels, &paths.corpus_dir)?;
    if queries.is_empty() || qrels.is_empty() {
        return Err(anyhow!("retrieval evaluation data is empty"));
    }
    let queries_sha256 = sha256_file(&paths.query_path)?;
    let qrels_sha256 = sha256_file(&paths.qrels_path)?;
    let corpus_manifest_hash = corpus_manifest_sha256(&paths.corpus_dir)?;
    let positive_judgment_count = qrels
        .values()
        .flat_map(|judgments| judgments.values())
        .filter(|grade| **grade > 0)
        .count();
    let explicit_negative_judgment_count = qrels
        .values()
        .flat_map(|judgments| judgments.values())
        .filter(|grade| **grade == 0)
        .count();
    let judgment_sources = judgment_source_labels(&queries);
    Ok(EvalData {
        queries,
        qrels,
        queries_sha256,
        qrels_sha256,
        corpus_manifest_sha256: corpus_manifest_hash,
        positive_judgment_count,
        explicit_negative_judgment_count,
        judgment_sources,
    })
}

fn judgment_source_labels(queries: &[EvalQuery]) -> Vec<String> {
    let sources = queries
        .iter()
        .filter_map(|query| query.judgment_source.as_deref())
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if sources.is_empty() {
        vec!["unspecified".to_string()]
    } else {
        sources
    }
}

/// The in-memory index of the corpus and the timings recorded while building it.
struct IndexedCorpus {
    store: KnowledgeStore,
    sync: SyncResult,
    indexing_ms: f64,
}

fn index_corpus(embedder: Embedder, corpus_dir: &Path) -> Result<IndexedCorpus> {
    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(embedder));
    store.add_source(
        "retrieval-eval",
        &corpus_dir.to_string_lossy(),
        &[],
        &["frontmatter".into()],
    )?;
    let source = store
        .get_source("retrieval-eval")?
        .ok_or_else(|| anyhow!("could not load evaluation source"))?;
    let enrichment_fn =
        knowledge::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrichment_ref = enrichment_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    let index_start = Instant::now();
    let sync = knowledge::sync::sync_source(&store, &source, enrichment_ref)?;
    let indexing_ms = index_start.elapsed().as_secs_f64() * 1000.0;
    Ok(IndexedCorpus {
        store,
        sync,
        indexing_ms,
    })
}

/// Settings shared by every query in one evaluation run.
struct RunContext<'a> {
    store: &'a KnowledgeStore,
    corpus_dir: &'a Path,
    hybrid_weights: &'a SearchWeights,
    layer_policy: LayerPolicy,
    repetitions: usize,
}

/// Raw search output for one query, before any scoring.
struct QuerySearches {
    layers: Vec<LayerReport>,
    results: Vec<KnowledgeSearchResult>,
    lexical_results: Vec<KnowledgeSearchResult>,
    vector_results: Vec<KnowledgeSearchResult>,
    query_times: Vec<Duration>,
}

/// Ranked file paths and quality metrics for one query under each channel.
struct QueryScores {
    hybrid_paths: Vec<String>,
    lexical_paths: Vec<String>,
    vector_paths: Vec<String>,
    hybrid: QualityMetrics,
    lexical: QualityMetrics,
    vector: QualityMetrics,
}

/// Citation checks accumulated across every returned result.
#[derive(Default)]
struct CitationTally {
    checked: usize,
    valid: usize,
    invalid: Vec<CitationIssue>,
    content_matches: usize,
    content_mismatches: Vec<CitationIssue>,
}

impl CitationTally {
    fn content_validity(&self) -> Option<f64> {
        (self.checked > 0).then_some(self.content_matches as f64 / self.checked as f64)
    }

    fn validity(&self) -> f64 {
        if self.checked == 0 {
            1.0
        } else {
            self.valid as f64 / self.checked as f64
        }
    }
}

/// Run-wide totals, updated once per query.
#[derive(Default)]
struct RetrievalTally {
    overall: MetricSums,
    with_relations: MetricSums,
    queries_with_neighbors: usize,
    lexical_only: MetricSums,
    vector_only: MetricSums,
    categories: BTreeMap<String, MetricSums>,
    languages: BTreeMap<String, MetricSums>,
    unanswerable_hybrid: UnanswerableMetricSums,
    unanswerable_lexical: UnanswerableMetricSums,
    unanswerable_vector: UnanswerableMetricSums,
    cases: Vec<EvalCaseReport>,
    latencies: Vec<Duration>,
    citations: CitationTally,
    filter_leaks: usize,
}

impl RetrievalTally {
    fn with_capacity(query_count: usize, repetitions: usize) -> Self {
        Self {
            cases: Vec::with_capacity(query_count),
            latencies: Vec::with_capacity(query_count * repetitions),
            ..Self::default()
        }
    }
}

fn evaluate_query(
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

fn query_scope(query: &EvalQuery, layer_policy: LayerPolicy) -> SearchScope<'_> {
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

/// Runs the warm-up, lexical, vector, and timed hybrid searches, in that order.
fn run_searches(
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

fn file_paths(results: &[KnowledgeSearchResult]) -> Vec<String> {
    results
        .iter()
        .map(|result| result.file_path.clone())
        .collect()
}

fn score_query(
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

/// Measures the shipped rule: linked files of the top hit are merged into the ranking.
fn record_relation_neighbors(
    store: &KnowledgeStore,
    results: &[KnowledgeSearchResult],
    judgments: &BTreeMap<String, u8>,
    tally: &mut RetrievalTally,
) -> Result<()> {
    let expanded = crate::search::attach_relation_neighbors(
        store,
        results.to_vec(),
        knowledge::links::RELATION_NEIGHBOR_LIMIT,
        TOP_K,
    )?;
    if expanded.len() != results.len() || expanded.iter().any(|result| result.why.graph_neighbor) {
        tally.queries_with_neighbors += 1;
    }
    tally
        .with_relations
        .push(&measure_query(&file_paths(&expanded), judgments));
    Ok(())
}

/// Adds the query to the answerable or unanswerable totals. Returns the
/// unanswerable metrics for an unanswerable query.
fn record_query_totals(
    tally: &mut RetrievalTally,
    query: &EvalQuery,
    scores: &QueryScores,
    judgments: &BTreeMap<String, u8>,
) -> Option<UnanswerableMetrics> {
    if query.answerability == QueryAnswerability::UnanswerableInCorpus {
        tally
            .unanswerable_hybrid
            .push(&scores.hybrid_paths, judgments);
        tally
            .unanswerable_lexical
            .push(&scores.lexical_paths, judgments);
        tally
            .unanswerable_vector
            .push(&scores.vector_paths, judgments);
        Some(measure_unanswerable_query(&scores.hybrid_paths, judgments))
    } else {
        tally.overall.push(&scores.hybrid);
        tally.lexical_only.push(&scores.lexical);
        tally.vector_only.push(&scores.vector);
        tally
            .categories
            .entry(query.category.clone())
            .or_default()
            .push(&scores.hybrid);
        tally
            .languages
            .entry(query.language.clone())
            .or_default()
            .push(&scores.hybrid);
        None
    }
}

/// Hybrid ranking to a deeper cutoff, from the same candidate pool as the TOP_K search.
fn search_deep_paths(
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

fn build_case(
    query: &EvalQuery,
    scores: QueryScores,
    unanswerable_hybrid: Option<UnanswerableMetrics>,
    deep_paths: Vec<String>,
    layers: Vec<LayerReport>,
) -> EvalCaseReport {
    let is_answerable = query.answerability == QueryAnswerability::Answerable;
    EvalCaseReport {
        query_id: query.query_id.clone(),
        category: query.category.clone(),
        language: query.language.clone(),
        answerability: query.answerability,
        answerability_evidence: query.answerability_evidence.clone(),
        hybrid: is_answerable.then_some(scores.hybrid),
        lexical_only: is_answerable.then_some(scores.lexical),
        vector_only: is_answerable.then_some(scores.vector),
        unanswerable_hybrid,
        distinct_files_at_10: unique_paths(&scores.hybrid_paths, TOP_K).len(),
        top_paths: unique_paths(&scores.hybrid_paths, TOP_K),
        lexical_top_paths: unique_paths(&scores.lexical_paths, TOP_K),
        vector_top_paths: unique_paths(&scores.vector_paths, TOP_K),
        deep_paths,
        layers,
    }
}

fn count_filter_leaks(results: &[KnowledgeSearchResult], expected_language: Option<&str>) -> usize {
    let Some(expected_language) = expected_language else {
        return 0;
    };
    results
        .iter()
        .filter(|result| {
            let frontmatter_language = result.metadata["frontmatter"]["language"].as_str();
            !crate::language::matches_language(
                expected_language,
                Path::new(&result.file_path),
                frontmatter_language,
            )
        })
        .count()
}

fn record_citations(
    corpus_dir: &Path,
    query_id: &str,
    results: &[KnowledgeSearchResult],
    citations: &mut CitationTally,
) -> Result<()> {
    for result in results {
        citations.checked += 1;
        let (citation_valid, content_matches) = citation_quality(
            corpus_dir,
            &result.file_path,
            &result.lines,
            &result.snippet,
        )?;
        let issue = || CitationIssue {
            query_id: query_id.to_string(),
            path: result.file_path.clone(),
            lines: result.lines.clone(),
        };
        if citation_valid {
            citations.valid += 1;
        } else {
            citations.invalid.push(issue());
        }
        if content_matches {
            citations.content_matches += 1;
        } else {
            citations.content_mismatches.push(issue());
        }
    }
    Ok(())
}

/// Fails if the dataset or corpus changed while the evaluation was running.
fn verify_inputs_unchanged(paths: &EvalPaths, data: &EvalData) -> Result<()> {
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
