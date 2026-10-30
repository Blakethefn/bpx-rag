use super::*;
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use crate::knowledge::query::SearchWeights;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Deserialize, Clone)]
pub(super) struct EvalQuery {
    pub(super) query_id: String,
    pub(super) text: String,
    pub(super) category: String,
    pub(super) language: String,
    #[serde(default)]
    pub(super) filter: Option<String>,
    #[serde(default)]
    pub(super) expected_language: Option<String>,
    #[serde(default)]
    pub(super) judgment_source: Option<String>,
    #[serde(default)]
    pub(super) answerability: QueryAnswerability,
    #[serde(default)]
    pub(super) answerability_evidence: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswerability {
    #[default]
    Answerable,
    UnanswerableInCorpus,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Qrel {
    pub(super) query_id: String,
    pub(super) path: String,
    pub(super) grade: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityMetrics {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub precision_at_5: f64,
    pub precision_at_10: f64,
    pub judged_coverage_at_5: f64,
    pub judged_coverage_at_10: f64,
    pub mrr_at_10: f64,
    pub ndcg_at_10: f64,
    pub success_at_5: f64,
}

#[derive(Debug, Serialize)]
pub struct RetrievalEvalReport {
    pub model_name: String,
    pub model_id: String,
    pub embedding_device: String,
    pub dataset_name: String,
    pub corpus_root_name: String,
    pub embedding_dimension: usize,
    pub batch_size: usize,
    pub intra_threads: usize,
    pub candidate_oversample_factor: u32,
    pub hybrid_keyword_weight: f64,
    pub hybrid_vector_weight: f64,
    pub queries_sha256: String,
    pub qrels_sha256: String,
    pub corpus_manifest_sha256: String,
    pub positive_judgment_count: usize,
    pub explicit_negative_judgment_count: usize,
    pub judgment_sources: Vec<String>,
    pub operating_system: String,
    pub architecture: String,
    pub host_threads: usize,
    pub query_count: usize,
    pub answerable_query_count: usize,
    pub unanswerable_query_count: usize,
    pub corpus_file_count: usize,
    pub sensitive_files_skipped: usize,
    pub chunk_count: usize,
    pub tree_sitter_files_indexed: usize,
    pub existing_chunker_files_indexed: usize,
    pub tree_sitter_chunks_indexed: usize,
    pub existing_chunker_chunks_indexed: usize,
    pub repetitions: usize,
    pub model_load_ms: f64,
    pub indexing_ms: f64,
    pub database_bytes: u64,
    pub model_artifact_bytes: u64,
    pub embedding_ms: u64,
    pub embedding_calls: usize,
    pub embedding_chunks: usize,
    pub sqlite_transaction_ms: u64,
    pub warm_query_p50_ms: f64,
    pub warm_query_p95_ms: f64,
    pub overall: QualityMetrics,
    pub lexical_only: QualityMetrics,
    pub vector_only: QualityMetrics,
    pub unanswerable_hybrid: UnanswerableMetrics,
    pub unanswerable_lexical_only: UnanswerableMetrics,
    pub unanswerable_vector_only: UnanswerableMetrics,
    pub cases: Vec<EvalCaseReport>,
    pub by_category: BTreeMap<String, QualityMetrics>,
    pub by_language: BTreeMap<String, QualityMetrics>,
    /// Fraction of snippets whose text matches the whitespace-normalized prefix of
    /// cited source lines.
    pub citation_content_validity: Option<f64>,
    pub citation_content_matches: usize,
    pub citation_content_mismatches: Vec<CitationIssue>,
    pub citation_validity: f64,
    pub citation_count: usize,
    pub invalid_citations: Vec<CitationIssue>,
    pub filter_leaks: usize,
}

#[derive(Debug, Serialize)]
pub struct EvalCaseReport {
    pub query_id: String,
    pub category: String,
    pub language: String,
    pub answerability: QueryAnswerability,
    pub answerability_evidence: Option<String>,
    pub hybrid: Option<QualityMetrics>,
    pub lexical_only: Option<QualityMetrics>,
    pub vector_only: Option<QualityMetrics>,
    pub unanswerable_hybrid: Option<UnanswerableMetrics>,
    pub distinct_files_at_10: usize,
    pub top_paths: Vec<String>,
    pub lexical_top_paths: Vec<String>,
    pub vector_top_paths: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct UnanswerableMetrics {
    pub query_count: usize,
    /// Fraction of top-10 returned files explicitly judged nonrelevant.
    pub explicit_negative_share_at_10: f64,
    pub judged_coverage_at_10: f64,
    /// Fraction of queries for which retrieval returned no files.
    pub empty_result_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct CitationIssue {
    pub query_id: String,
    pub path: String,
    pub lines: String,
}

#[derive(Clone, Copy, Default)]
pub(super) struct MetricSums {
    pub(super) recall_at_5: f64,
    pub(super) recall_at_10: f64,
    pub(super) precision_at_5: f64,
    pub(super) precision_at_10: f64,
    pub(super) judged_coverage_at_5: f64,
    pub(super) judged_coverage_at_10: f64,
    pub(super) mrr_at_10: f64,
    pub(super) ndcg_at_10: f64,
    pub(super) success_at_5: f64,
    pub(super) count: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct UnanswerableMetricSums {
    pub(super) explicit_negative_share_at_10: f64,
    pub(super) judged_coverage_at_10: f64,
    pub(super) empty_result_rate: f64,
    pub(super) count: usize,
}

impl UnanswerableMetricSums {
    fn push(&mut self, ranked_paths: &[String], judgments: &BTreeMap<String, u8>) {
        let metrics = measure_unanswerable_query(ranked_paths, judgments);
        self.explicit_negative_share_at_10 += metrics.explicit_negative_share_at_10;
        self.judged_coverage_at_10 += metrics.judged_coverage_at_10;
        self.empty_result_rate += metrics.empty_result_rate;
        self.count += 1;
    }

    fn average(self) -> UnanswerableMetrics {
        let count = self.count.max(1) as f64;
        UnanswerableMetrics {
            query_count: self.count,
            explicit_negative_share_at_10: self.explicit_negative_share_at_10 / count,
            judged_coverage_at_10: self.judged_coverage_at_10 / count,
            empty_result_rate: self.empty_result_rate / count,
        }
    }
}

pub(super) fn measure_unanswerable_query(
    ranked_paths: &[String],
    judgments: &BTreeMap<String, u8>,
) -> UnanswerableMetrics {
    let top_paths = unique_paths(ranked_paths, TOP_K);
    let returned = top_paths.len();
    let explicitly_negative = top_paths
        .iter()
        .filter(|path| judgments.get(path.as_str()) == Some(&0))
        .count();
    let judged = top_paths
        .iter()
        .filter(|path| judgments.contains_key(path.as_str()))
        .count();
    UnanswerableMetrics {
        query_count: 1,
        explicit_negative_share_at_10: if returned == 0 {
            0.0
        } else {
            explicitly_negative as f64 / returned as f64
        },
        judged_coverage_at_10: if returned == 0 {
            0.0
        } else {
            judged as f64 / returned as f64
        },
        empty_result_rate: (returned == 0) as u8 as f64,
    }
}

impl MetricSums {
    fn push(&mut self, metrics: &QualityMetrics) {
        self.recall_at_5 += metrics.recall_at_5;
        self.recall_at_10 += metrics.recall_at_10;
        self.precision_at_5 += metrics.precision_at_5;
        self.precision_at_10 += metrics.precision_at_10;
        self.judged_coverage_at_5 += metrics.judged_coverage_at_5;
        self.judged_coverage_at_10 += metrics.judged_coverage_at_10;
        self.mrr_at_10 += metrics.mrr_at_10;
        self.ndcg_at_10 += metrics.ndcg_at_10;
        self.success_at_5 += metrics.success_at_5;
        self.count += 1;
    }

    fn average(self) -> QualityMetrics {
        let count = self.count.max(1) as f64;
        QualityMetrics {
            recall_at_5: self.recall_at_5 / count,
            recall_at_10: self.recall_at_10 / count,
            precision_at_5: self.precision_at_5 / count,
            precision_at_10: self.precision_at_10 / count,
            judged_coverage_at_5: self.judged_coverage_at_5 / count,
            judged_coverage_at_10: self.judged_coverage_at_10 / count,
            mrr_at_10: self.mrr_at_10 / count,
            ndcg_at_10: self.ndcg_at_10 / count,
            success_at_5: self.success_at_5 / count,
        }
    }
}

pub fn run_retrieval_evaluation(
    repetitions: usize,
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
    hybrid_weights: SearchWeights,
) -> Result<RetrievalEvalReport> {
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
    let config = Config::load()?;
    let model_dir = crate::app::model_directory(&config);
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
    let queries = read_jsonl::<EvalQuery>(&query_path)?;
    let qrels = read_qrels(&qrels_path)?;
    validate_qrels(&queries, &qrels, &corpus_dir)?;
    if queries.is_empty() || qrels.is_empty() {
        return Err(anyhow!("retrieval evaluation data is empty"));
    }
    let queries_sha256 = sha256_file(&query_path)?;
    let qrels_sha256 = sha256_file(&qrels_path)?;
    let corpus_manifest_hash = corpus_manifest_sha256(&corpus_dir)?;
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
    let mut judgment_sources = queries
        .iter()
        .filter_map(|query| query.judgment_source.as_deref())
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if judgment_sources.is_empty() {
        judgment_sources.push("unspecified".to_string());
    }

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

    let mut overall = MetricSums::default();
    let mut lexical_only = MetricSums::default();
    let mut vector_only = MetricSums::default();
    let mut categories: BTreeMap<String, MetricSums> = BTreeMap::new();
    let mut languages: BTreeMap<String, MetricSums> = BTreeMap::new();
    let mut unanswerable_hybrid = UnanswerableMetricSums::default();
    let mut unanswerable_lexical = UnanswerableMetricSums::default();
    let mut unanswerable_vector = UnanswerableMetricSums::default();
    let mut cases = Vec::with_capacity(queries.len());
    let mut latencies = Vec::with_capacity(queries.len() * repetitions);
    let mut citations_checked = 0usize;
    let mut valid_citations = 0usize;
    let mut invalid_citations = Vec::new();
    let mut citation_content_matches = 0usize;
    let mut citation_content_mismatches = Vec::new();
    let mut filter_leaks = 0usize;

    for query in &queries {
        let judgments = qrels
            .get(&query.query_id)
            .ok_or_else(|| anyhow!("no relevance judgments for query {}", query.query_id))?;
        let _ = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &hybrid_weights,
            1024,
        )?;
        let lexical_results = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &SearchWeights {
                keyword_weight: 1.0,
                vector_weight: 0.0,
            },
            1024,
        )?;
        let vector_results = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &SearchWeights {
                keyword_weight: 0.0,
                vector_weight: 1.0,
            },
            1024,
        )?;
        let mut results = Vec::new();
        let mut query_times = Vec::with_capacity(repetitions);
        for _ in 0..repetitions {
            let started = Instant::now();
            results = knowledge::query::knowledge_search(
                &store,
                &query.text,
                query.filter.as_deref(),
                Some("retrieval-eval"),
                TOP_K as u32,
                &hybrid_weights,
                1024,
            )?;
            query_times.push(started.elapsed());
        }
        latencies.extend(query_times.iter().copied());

        let ranked_paths = results
            .iter()
            .map(|result| result.file_path.clone())
            .collect::<Vec<_>>();
        let metrics = measure_query(&ranked_paths, judgments);
        let lexical_metrics = measure_query(
            &lexical_results
                .iter()
                .map(|result| result.file_path.clone())
                .collect::<Vec<_>>(),
            judgments,
        );
        let vector_metrics = measure_query(
            &vector_results
                .iter()
                .map(|result| result.file_path.clone())
                .collect::<Vec<_>>(),
            judgments,
        );
        let no_answer_metrics = if query.answerability == QueryAnswerability::UnanswerableInCorpus {
            unanswerable_hybrid.push(&ranked_paths, judgments);
            unanswerable_lexical.push(
                &lexical_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                judgments,
            );
            unanswerable_vector.push(
                &vector_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                judgments,
            );
            Some(measure_unanswerable_query(&ranked_paths, judgments))
        } else {
            overall.push(&metrics);
            lexical_only.push(&lexical_metrics);
            vector_only.push(&vector_metrics);
            categories
                .entry(query.category.clone())
                .or_default()
                .push(&metrics);
            languages
                .entry(query.language.clone())
                .or_default()
                .push(&metrics);
            None
        };
        let is_answerable = query.answerability == QueryAnswerability::Answerable;
        cases.push(EvalCaseReport {
            query_id: query.query_id.clone(),
            category: query.category.clone(),
            language: query.language.clone(),
            answerability: query.answerability,
            answerability_evidence: query.answerability_evidence.clone(),
            hybrid: is_answerable.then_some(metrics.clone()),
            lexical_only: is_answerable.then_some(lexical_metrics),
            vector_only: is_answerable.then_some(vector_metrics),
            unanswerable_hybrid: no_answer_metrics,
            distinct_files_at_10: unique_paths(&ranked_paths, TOP_K).len(),
            top_paths: unique_paths(&ranked_paths, TOP_K),
            lexical_top_paths: unique_paths(
                &lexical_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            ),
            vector_top_paths: unique_paths(
                &vector_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            ),
        });

        if let Some(expected_language) = &query.expected_language {
            for result in &results {
                let frontmatter_language = result.metadata["frontmatter"]["language"].as_str();
                if !crate::language::matches_language(
                    expected_language,
                    Path::new(&result.file_path),
                    frontmatter_language,
                ) {
                    filter_leaks += 1;
                }
            }
        }

        for result in &results {
            citations_checked += 1;
            let (citation_valid, content_matches) = citation_quality(
                &corpus_dir,
                &result.file_path,
                &result.lines,
                &result.snippet,
            )?;
            if citation_valid {
                valid_citations += 1;
            } else {
                invalid_citations.push(CitationIssue {
                    query_id: query.query_id.clone(),
                    path: result.file_path.clone(),
                    lines: result.lines.clone(),
                });
            }
            if content_matches {
                citation_content_matches += 1;
            } else {
                citation_content_mismatches.push(CitationIssue {
                    query_id: query.query_id.clone(),
                    path: result.file_path.clone(),
                    lines: result.lines.clone(),
                });
            }
        }
    }

    if sha256_file(&query_path)? != queries_sha256 || sha256_file(&qrels_path)? != qrels_sha256 {
        anyhow::bail!("retrieval evaluation dataset changed while it was running; retry");
    }
    if corpus_manifest_sha256(&corpus_dir)? != corpus_manifest_hash {
        anyhow::bail!("retrieval corpus changed while evaluation was running; retry");
    }

    latencies.sort_by(duration_cmp);
    let latency_ms = latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    let db_bytes = in_memory_database_bytes(store.conn())?;

    Ok(RetrievalEvalReport {
        model_name: config.embeddings.model,
        model_id,
        embedding_device: config.embeddings.device.as_str().to_string(),
        dataset_name: data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("dataset")
            .to_string(),
        corpus_root_name: corpus_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("corpus")
            .to_string(),
        embedding_dimension,
        batch_size: config.embeddings.batch_size,
        intra_threads: config.embeddings.intra_threads,
        candidate_oversample_factor: knowledge::query::CANDIDATE_OVERSAMPLE_FACTOR,
        hybrid_keyword_weight: hybrid_weights.keyword_weight,
        hybrid_vector_weight: hybrid_weights.vector_weight,
        queries_sha256,
        qrels_sha256,
        corpus_manifest_sha256: corpus_manifest_hash,
        positive_judgment_count,
        explicit_negative_judgment_count,
        judgment_sources,
        operating_system: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        host_threads: std::thread::available_parallelism().map_or(1, usize::from),
        query_count: queries.len(),
        answerable_query_count: overall.count,
        unanswerable_query_count: unanswerable_hybrid.count,
        corpus_file_count: sync.files_scanned,
        sensitive_files_skipped: sync.sensitive_files_skipped,
        chunk_count: sync.chunks_total,
        tree_sitter_files_indexed: sync.tree_sitter_files_indexed,
        existing_chunker_files_indexed: sync.existing_chunker_files_indexed,
        tree_sitter_chunks_indexed: sync.tree_sitter_chunks_indexed,
        existing_chunker_chunks_indexed: sync.existing_chunker_chunks_indexed,
        repetitions,
        model_load_ms,
        indexing_ms,
        database_bytes: db_bytes,
        model_artifact_bytes,
        embedding_ms: sync.embedding_ms,
        embedding_calls: sync.embedding_calls,
        embedding_chunks: sync.embedding_chunks,
        sqlite_transaction_ms: sync.transaction_ms,
        warm_query_p50_ms: percentile(&latency_ms, 0.50),
        warm_query_p95_ms: percentile(&latency_ms, 0.95),
        overall: overall.average(),
        lexical_only: lexical_only.average(),
        vector_only: vector_only.average(),
        unanswerable_hybrid: unanswerable_hybrid.average(),
        unanswerable_lexical_only: unanswerable_lexical.average(),
        unanswerable_vector_only: unanswerable_vector.average(),
        cases,
        by_category: categories
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        by_language: languages
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        citation_content_validity: (citations_checked > 0)
            .then_some(citation_content_matches as f64 / citations_checked as f64),
        citation_content_matches,
        citation_content_mismatches,
        citation_validity: if citations_checked == 0 {
            1.0
        } else {
            valid_citations as f64 / citations_checked as f64
        },
        citation_count: citations_checked,
        invalid_citations,
        filter_leaks,
    })
}

pub(super) fn read_qrels(path: &Path) -> Result<HashMap<String, BTreeMap<String, u8>>> {
    let records = read_jsonl::<Qrel>(path)?;
    let mut judgments: HashMap<String, BTreeMap<String, u8>> = HashMap::new();
    let mut seen_records = std::collections::HashSet::new();
    for record in records {
        if record.query_id.trim().is_empty() || record.path.trim().is_empty() {
            anyhow::bail!("relevance judgments require a query ID and corpus path");
        }
        if record.grade > 3 {
            anyhow::bail!(
                "relevance grade must be between 0 and 3 for query {} and path {}",
                record.query_id,
                record.path
            );
        }
        if !seen_records.insert((record.query_id.clone(), record.path.clone())) {
            anyhow::bail!(
                "duplicate relevance judgment for query {} and path {}",
                record.query_id,
                record.path
            );
        }
        judgments
            .entry(record.query_id)
            .or_default()
            .insert(record.path, record.grade);
    }
    Ok(judgments)
}

pub(super) fn validate_qrels(
    queries: &[EvalQuery],
    qrels: &HashMap<String, BTreeMap<String, u8>>,
    corpus_dir: &Path,
) -> Result<()> {
    let query_ids = validate_queries(queries)?;
    if let Some(query_id) = qrels
        .keys()
        .find(|query_id| !query_ids.contains(query_id.as_str()))
    {
        anyhow::bail!("relevance judgments reference unknown query {query_id}");
    }
    let indexable_paths = crate::knowledge::sync::walk_source_dir(corpus_dir, &[])?
        .into_keys()
        .collect::<std::collections::HashSet<_>>();
    for query in queries {
        let judgments = qrels.get(&query.query_id);
        let has_positive =
            judgments.is_some_and(|judgments| judgments.values().any(|grade| *grade > 0));
        match query.answerability {
            QueryAnswerability::Answerable if !has_positive => anyhow::bail!(
                "answerable query {} has no positive relevance judgments",
                query.query_id
            ),
            QueryAnswerability::UnanswerableInCorpus if has_positive => anyhow::bail!(
                "unanswerable query {} has a positive relevance judgment",
                query.query_id
            ),
            QueryAnswerability::UnanswerableInCorpus => {
                let judgments = judgments.cloned().unwrap_or_default();
                let unjudged_count = indexable_paths
                    .iter()
                    .filter(|path| !judgments.contains_key(*path))
                    .count();
                if unjudged_count > 0 {
                    anyhow::bail!(
                        "unanswerable query {} must have explicit grade-0 judgments for every indexable corpus file; {} files are unjudged",
                        query.query_id,
                        unjudged_count
                    );
                }
                if judgments.values().any(|grade| *grade != 0) {
                    anyhow::bail!(
                        "unanswerable query {} must have only explicit grade-0 judgments",
                        query.query_id
                    );
                }
            }
            _ => {}
        }
    }
    let canonical_root = corpus_dir.canonicalize().with_context(|| {
        format!(
            "failed to resolve corpus directory {}",
            corpus_dir.display()
        )
    })?;
    for path in qrels.values().flat_map(|judgments| judgments.keys()) {
        let relative = Path::new(path);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            anyhow::bail!("relevance judgment path is not a safe relative path: {path}");
        }
        let resolved = canonical_root
            .join(relative)
            .canonicalize()
            .with_context(|| format!("judged corpus file does not exist: {path}"))?;
        if !resolved.starts_with(&canonical_root) || !resolved.is_file() {
            anyhow::bail!("judged corpus path is outside the corpus or not a file: {path}");
        }
        if !indexable_paths.contains(path) {
            anyhow::bail!("judged corpus file is excluded by indexing rules: {path}");
        }
    }
    Ok(())
}

pub(super) fn validate_queries(queries: &[EvalQuery]) -> Result<std::collections::HashSet<String>> {
    let mut query_ids = std::collections::HashSet::new();
    for query in queries {
        if query.query_id.trim().is_empty()
            || query.text.trim().is_empty()
            || query.category.trim().is_empty()
            || query.language.trim().is_empty()
        {
            anyhow::bail!("evaluation queries require nonempty ID, text, category, and language");
        }
        if !query_ids.insert(query.query_id.clone()) {
            anyhow::bail!("duplicate evaluation query ID: {}", query.query_id);
        }
        if query.answerability == QueryAnswerability::UnanswerableInCorpus
            && query
                .answerability_evidence
                .as_deref()
                .is_none_or(|evidence| evidence.trim().is_empty())
        {
            anyhow::bail!(
                "unanswerable query {} requires answerability_evidence describing exhaustive corpus review",
                query.query_id
            );
        }
    }
    Ok(query_ids)
}

pub(super) fn measure_query(
    ranked_paths: &[String],
    judgments: &BTreeMap<String, u8>,
) -> QualityMetrics {
    let unique_paths = unique_paths(ranked_paths, usize::MAX);
    let relevant_total = judgments
        .values()
        .filter(|grade| **grade > 0)
        .count()
        .max(1) as f64;
    let relevant_at = |cutoff: usize| {
        unique_paths
            .iter()
            .take(cutoff)
            .filter(|path| judgments.get(path.as_str()).is_some_and(|grade| *grade > 0))
            .count()
    };
    let judged_coverage_at = |cutoff: usize| {
        let returned = unique_paths.len().min(cutoff);
        if returned == 0 {
            return 0.0;
        }
        let judged = unique_paths
            .iter()
            .take(cutoff)
            .filter(|path| judgments.contains_key(path.as_str()))
            .count();
        judged as f64 / returned as f64
    };
    let mrr_at_10 = unique_paths
        .iter()
        .take(TOP_K)
        .position(|path| judgments.get(path.as_str()).is_some_and(|grade| *grade > 0))
        .map_or(0.0, |rank| 1.0 / (rank as f64 + 1.0));
    let actual_dcg = dcg(unique_paths
        .iter()
        .take(TOP_K)
        .map(|path| judgments.get(path.as_str()).copied().unwrap_or(0)));
    let mut ideal_grades = judgments
        .values()
        .copied()
        .filter(|grade| *grade > 0)
        .collect::<Vec<_>>();
    ideal_grades.sort_by(|left, right| right.cmp(left));
    let ideal_dcg = dcg(ideal_grades.into_iter().take(TOP_K));
    let found_at_5 = relevant_at(5);
    QualityMetrics {
        recall_at_5: found_at_5 as f64 / relevant_total,
        recall_at_10: relevant_at(TOP_K) as f64 / relevant_total,
        precision_at_5: found_at_5 as f64 / 5.0,
        precision_at_10: relevant_at(TOP_K) as f64 / TOP_K as f64,
        judged_coverage_at_5: judged_coverage_at(5),
        judged_coverage_at_10: judged_coverage_at(TOP_K),
        mrr_at_10,
        ndcg_at_10: if ideal_dcg == 0.0 {
            0.0
        } else {
            actual_dcg / ideal_dcg
        },
        success_at_5: if found_at_5 > 0 { 1.0 } else { 0.0 },
    }
}

pub(super) fn unique_paths(ranked_paths: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ranked_paths
        .iter()
        .filter(|path| seen.insert(path.as_str()))
        .take(limit)
        .cloned()
        .collect()
}

pub(super) fn dcg(grades: impl Iterator<Item = u8>) -> f64 {
    grades
        .enumerate()
        .map(|(rank, grade)| {
            let gain = 2_f64.powi(i32::from(grade)) - 1.0;
            gain / ((rank as f64 + 2.0).log2())
        })
        .sum()
}

pub(super) fn citation_quality(
    corpus_dir: &Path,
    file_path: &str,
    range: &str,
    snippet: &str,
) -> Result<(bool, bool)> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Ok((false, false));
    }
    let file_path = corpus_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("search result cited a missing file {}", file_path.display()))?;
    let Some((start, end)) = range.split_once('-') else {
        return Ok((false, false));
    };
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return Ok((false, false));
    };
    let lines = content.lines().collect::<Vec<_>>();
    if start == 0 || end < start || end > lines.len() as u32 {
        return Ok((false, false));
    }
    let cited_lines = lines[(start - 1) as usize..end as usize].join("\n");
    let snippet = snippet.strip_suffix("...").unwrap_or(snippet);
    let normalized_citation = normalize_citation_text(&cited_lines);
    let normalized_snippet = normalize_citation_text(snippet);
    let content_matches =
        !normalized_snippet.is_empty() && normalized_citation.starts_with(&normalized_snippet);
    Ok((true, content_matches))
}

pub(super) fn normalize_citation_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
