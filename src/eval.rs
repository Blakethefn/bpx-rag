use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::embedder::{self, Embed, Embedder};
use crate::knowledge;
use crate::store::search::SearchWeights;

const TOP_K: usize = 10;

#[derive(Debug, Deserialize, Clone)]
struct EvalQuery {
    query_id: String,
    text: String,
    category: String,
    language: String,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    expected_language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Qrel {
    query_id: String,
    path: String,
    grade: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityMetrics {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub precision_at_5: f64,
    pub precision_at_10: f64,
    pub mrr_at_10: f64,
    pub ndcg_at_10: f64,
    pub success_at_5: f64,
}

#[derive(Debug, Serialize)]
pub struct RetrievalEvalReport {
    pub model_name: String,
    pub model_id: String,
    pub dataset_name: String,
    pub corpus_root_name: String,
    pub embedding_dimension: usize,
    pub operating_system: String,
    pub architecture: String,
    pub host_threads: usize,
    pub query_count: usize,
    pub corpus_file_count: usize,
    pub chunk_count: usize,
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
    pub cases: Vec<EvalCaseReport>,
    pub by_category: BTreeMap<String, QualityMetrics>,
    pub by_language: BTreeMap<String, QualityMetrics>,
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
    pub hybrid: QualityMetrics,
    pub lexical_only: QualityMetrics,
    pub vector_only: QualityMetrics,
    pub top_paths: Vec<String>,
    pub lexical_top_paths: Vec<String>,
    pub vector_top_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CitationIssue {
    pub query_id: String,
    pub path: String,
    pub lines: String,
}

#[derive(Debug, Serialize)]
pub struct ScalingPoint {
    pub chunk_count: usize,
    pub index_mode: &'static str,
    pub database_bytes: u64,
    pub indexing_ms: f64,
    pub ann_build_or_load_ms: f64,
    pub exact_query_p50_ms: f64,
    pub exact_query_p95_ms: f64,
    pub ann_query_p50_ms: f64,
    pub ann_query_p95_ms: f64,
    pub ann_recall_at_10: f64,
    pub ann_memory_bytes: usize,
    pub result_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ScalingEvalReport {
    pub embedding_dimension: usize,
    pub query_count: usize,
    pub repetitions: usize,
    pub points: Vec<ScalingPoint>,
}

#[derive(Clone, Copy, Default)]
struct MetricSums {
    recall_at_5: f64,
    recall_at_10: f64,
    precision_at_5: f64,
    precision_at_10: f64,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    success_at_5: f64,
    count: usize,
}

impl MetricSums {
    fn push(&mut self, metrics: &QualityMetrics) {
        self.recall_at_5 += metrics.recall_at_5;
        self.recall_at_10 += metrics.recall_at_10;
        self.precision_at_5 += metrics.precision_at_5;
        self.precision_at_10 += metrics.precision_at_10;
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
) -> Result<RetrievalEvalReport> {
    if !(1..=100).contains(&repetitions) {
        return Err(anyhow!("repetitions must be between 1 and 100"));
    }
    let config = Config::load()?;
    let model_dir = crate::rag_app::model_directory(&config);
    if !embedder::model_files_present(&model_dir, &config.embeddings.model) {
        return Err(anyhow!(
            "evaluation requires the configured model to be cached; run `bpx-rag models download` first ({}).",
            model_dir.display()
        ));
    }

    let model_start = Instant::now();
    let embedder = Embedder::load_cached(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
    )?;
    let model_load_ms = model_start.elapsed().as_secs_f64() * 1000.0;
    let model_id = embedder.model_id().to_string();
    let embedding_dimension = embedder.dim();
    let model_artifact_bytes = embedder::model_cache_bytes(&model_dir, &config.embeddings.model)?;

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
    let queries = read_jsonl::<EvalQuery>(&data_dir.join("queries.jsonl"))?;
    let qrels = read_qrels(&data_dir.join("qrels.jsonl"))?;
    validate_qrels(&queries, &qrels, &corpus_dir)?;
    if queries.is_empty() || qrels.is_empty() {
        return Err(anyhow!("retrieval evaluation data is empty"));
    }

    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(embedder));
    store.add_source(
        "retrieval-eval",
        &corpus_dir.to_string_lossy(),
        None,
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
    let mut cases = Vec::with_capacity(queries.len());
    let mut latencies = Vec::with_capacity(queries.len() * repetitions);
    let mut citations_checked = 0usize;
    let mut valid_citations = 0usize;
    let mut invalid_citations = Vec::new();
    let mut filter_leaks = 0usize;

    for query in &queries {
        let judgments = qrels
            .get(&query.query_id)
            .ok_or_else(|| anyhow!("no relevance judgments for query {}", query.query_id))?;
        let _ = knowledge::search::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &SearchWeights::default(),
            1024,
        )?;
        let lexical_results = knowledge::search::knowledge_search(
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
        let vector_results = knowledge::search::knowledge_search(
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
            results = knowledge::search::knowledge_search(
                &store,
                &query.text,
                query.filter.as_deref(),
                Some("retrieval-eval"),
                TOP_K as u32,
                &SearchWeights::default(),
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
        cases.push(EvalCaseReport {
            query_id: query.query_id.clone(),
            category: query.category.clone(),
            language: query.language.clone(),
            hybrid: metrics.clone(),
            lexical_only: lexical_metrics,
            vector_only: vector_metrics,
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
                let actual = result.metadata["frontmatter"]["language"].as_str();
                if actual != Some(expected_language.as_str()) {
                    filter_leaks += 1;
                }
            }
        }

        for result in &results {
            citations_checked += 1;
            if citation_is_valid(&corpus_dir, &result.file_path, &result.lines)? {
                valid_citations += 1;
            } else {
                invalid_citations.push(CitationIssue {
                    query_id: query.query_id.clone(),
                    path: result.file_path.clone(),
                    lines: result.lines.clone(),
                });
            }
        }
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
        operating_system: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        host_threads: std::thread::available_parallelism().map_or(1, usize::from),
        query_count: queries.len(),
        corpus_file_count: sync.files_scanned,
        chunk_count: sync.chunks_total,
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
        cases,
        by_category: categories
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        by_language: languages
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
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

pub fn run_scaling_evaluation(repetitions: usize) -> Result<ScalingEvalReport> {
    if !(1..=100).contains(&repetitions) {
        return Err(anyhow!("repetitions must be between 1 and 100"));
    }
    let sizes = [1_000, 10_000, 50_000];
    const QUERY_COUNT: usize = 5;
    let mut points = Vec::with_capacity(sizes.len());
    for chunk_count in sizes {
        points.push(run_scaling_point(chunk_count, QUERY_COUNT, repetitions)?);
    }
    Ok(ScalingEvalReport {
        embedding_dimension: ScalingEmbedder::DIMENSION,
        query_count: QUERY_COUNT,
        repetitions,
        points,
    })
}

fn run_scaling_point(
    chunk_count: usize,
    query_count: usize,
    repetitions: usize,
) -> Result<ScalingPoint> {
    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(ScalingEmbedder));
    store.add_source("scaling", "/synthetic/bpx-rag-scaling", None, &[])?;
    let source = store
        .get_source("scaling")?
        .ok_or_else(|| anyhow!("could not load synthetic scaling source"))?;

    let index_start = Instant::now();
    let conn = store.conn();
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let index_result = (|| -> Result<()> {
        for index in 0..chunk_count {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                 VALUES (?1, ?2, 'synthetic', 4096)",
                rusqlite::params![source.id, format!("src/chunk-{index}.rs")],
            )?;
            let file_id = conn.last_insert_rowid();
            let title = format!("synthetic item {index}");
            let content = format!(
                "Benchmark-only unrelated code note with stable filler terms for chunk {index}."
            );
            conn.execute(
                "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
                rusqlite::params![title, content],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
                rusqlite::params![rowid, title, content],
            )?;
            conn.execute(
                "INSERT INTO knowledge_chunk_meta
                 (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, '{}')",
                rusqlite::params![rowid, file_id, source.id, 1, 4],
            )?;
            let vector = ScalingEmbedder::vector_for(index);
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    rowid,
                    embedder::embedding_to_bytes(&vector),
                    ScalingEmbedder::DIMENSION as i64,
                    ScalingEmbedder::MODEL_ID
                ],
            )?;
        }
        Ok(())
    })();
    match index_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    let indexing_ms = index_start.elapsed().as_secs_f64() * 1000.0;
    let database_bytes = in_memory_database_bytes(conn)?;
    let weights = SearchWeights {
        keyword_weight: 0.0,
        vector_weight: 1.0,
    };
    let mut exact_latencies = Vec::with_capacity(query_count * repetitions);
    let mut exact_paths_by_query = Vec::with_capacity(query_count);
    for query_index in 0..query_count {
        let query = format!("synthetic semantic query {query_index}");
        let mut exact_paths = Vec::new();
        for _ in 0..repetitions {
            let started = Instant::now();
            let exact_results = knowledge::search::knowledge_search(
                &store,
                &query,
                None,
                Some("scaling"),
                TOP_K as u32,
                &weights,
                128,
            )?;
            exact_latencies.push(started.elapsed());
            exact_paths = unique_paths(
                &exact_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            );
        }
        exact_paths_by_query.push(exact_paths);
    }

    let ann_start = Instant::now();
    store.ensure_vector_index(ScalingEmbedder::MODEL_ID, ScalingEmbedder::DIMENSION)?;
    let ann_build_or_load_ms = ann_start.elapsed().as_secs_f64() * 1000.0;
    let ann_memory_bytes = store.vector_index_memory_usage();
    let index_mode = if ann_memory_bytes == 0 {
        "exact"
    } else {
        "hnsw"
    };
    let mut ann_latencies = Vec::with_capacity(query_count * repetitions);
    let mut recalls = Vec::with_capacity(query_count);
    let mut result_count = 0;
    for (query_index, exact_paths) in exact_paths_by_query.iter().enumerate() {
        let query = format!("synthetic semantic query {query_index}");
        let mut ann_paths = Vec::new();
        for _ in 0..repetitions {
            let started = Instant::now();
            let ann_results = knowledge::search::knowledge_search(
                &store,
                &query,
                None,
                Some("scaling"),
                TOP_K as u32,
                &weights,
                128,
            )?;
            ann_latencies.push(started.elapsed());
            result_count = ann_results.len();
            ann_paths = unique_paths(
                &ann_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            );
        }

        let exact_path_set = exact_paths.iter().collect::<std::collections::HashSet<_>>();
        recalls.push(if exact_paths.is_empty() {
            1.0
        } else {
            ann_paths
                .iter()
                .filter(|path| exact_path_set.contains(path))
                .count() as f64
                / exact_paths.len() as f64
        });
    }

    exact_latencies.sort_by(duration_cmp);
    ann_latencies.sort_by(duration_cmp);
    let exact_latency_ms = exact_latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    let ann_latency_ms = ann_latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    Ok(ScalingPoint {
        chunk_count,
        index_mode,
        database_bytes,
        indexing_ms,
        ann_build_or_load_ms,
        exact_query_p50_ms: percentile(&exact_latency_ms, 0.5),
        exact_query_p95_ms: percentile(&exact_latency_ms, 0.95),
        ann_query_p50_ms: percentile(&ann_latency_ms, 0.5),
        ann_query_p95_ms: percentile(&ann_latency_ms, 0.95),
        ann_recall_at_10: recalls.iter().sum::<f64>() / recalls.len().max(1) as f64,
        ann_memory_bytes,
        result_count,
    })
}

struct ScalingEmbedder;

impl ScalingEmbedder {
    const DIMENSION: usize = 384;
    const MODEL_ID: &'static str = "bpx-rag-scaling-benchmark-v1";

    fn vector_for(index: usize) -> Vec<f32> {
        let mut state = (index as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut vector = (0..Self::DIMENSION)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as f32 / ((1_u32 << 24) as f32)) - 0.5
            })
            .collect::<Vec<_>>();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        vector.iter_mut().for_each(|value| *value /= norm);
        vector
    }
}

impl embedder::Embed for ScalingEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        let query_index = _text
            .split_whitespace()
            .last()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        Ok(Self::vector_for(1_000_000 + query_index))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                let query_index = text
                    .split_whitespace()
                    .last()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                Self::vector_for(1_000_000 + query_index)
            })
            .collect())
    }

    fn dim(&self) -> usize {
        Self::DIMENSION
    }

    fn model_id(&self) -> &str {
        Self::MODEL_ID
    }
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    fs::read_to_string(path)
        .with_context(|| format!("failed to read evaluation data {}", path.display()))?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .with_context(|| format!("invalid JSONL record in {}", path.display()))
        })
        .collect()
}

fn read_qrels(path: &Path) -> Result<HashMap<String, BTreeMap<String, u8>>> {
    let records = read_jsonl::<Qrel>(path)?;
    let mut judgments: HashMap<String, BTreeMap<String, u8>> = HashMap::new();
    let mut seen_records = std::collections::HashSet::new();
    for record in records {
        if !seen_records.insert((record.query_id.clone(), record.path.clone())) {
            anyhow::bail!(
                "duplicate relevance judgment for query {} and path {}",
                record.query_id,
                record.path
            );
        }
        if record.grade == 0 {
            continue;
        }
        judgments
            .entry(record.query_id)
            .or_default()
            .insert(record.path, record.grade);
    }
    Ok(judgments)
}

fn validate_qrels(
    queries: &[EvalQuery],
    qrels: &HashMap<String, BTreeMap<String, u8>>,
    corpus_dir: &Path,
) -> Result<()> {
    let query_ids = queries
        .iter()
        .map(|query| query.query_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if let Some(query_id) = qrels
        .keys()
        .find(|query_id| !query_ids.contains(query_id.as_str()))
    {
        anyhow::bail!("relevance judgments reference unknown query {query_id}");
    }
    for query in queries {
        if qrels
            .get(&query.query_id)
            .map_or(true, |judgments| judgments.is_empty())
        {
            anyhow::bail!(
                "query {} has no positive relevance judgments",
                query.query_id
            );
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
    }
    Ok(())
}

fn measure_query(ranked_paths: &[String], judgments: &BTreeMap<String, u8>) -> QualityMetrics {
    let unique_paths = unique_paths(ranked_paths, usize::MAX);
    let relevant_total = judgments.len().max(1) as f64;
    let relevant_at = |cutoff: usize| {
        unique_paths
            .iter()
            .take(cutoff)
            .filter(|path| judgments.contains_key(path.as_str()))
            .count()
    };
    let mrr_at_10 = unique_paths
        .iter()
        .take(TOP_K)
        .position(|path| judgments.contains_key(path.as_str()))
        .map_or(0.0, |rank| 1.0 / (rank as f64 + 1.0));
    let actual_dcg = dcg(unique_paths
        .iter()
        .take(TOP_K)
        .map(|path| judgments.get(path.as_str()).copied().unwrap_or(0)));
    let mut ideal_grades = judgments.values().copied().collect::<Vec<_>>();
    ideal_grades.sort_by(|left, right| right.cmp(left));
    let ideal_dcg = dcg(ideal_grades.into_iter().take(TOP_K));
    let found_at_5 = relevant_at(5);
    QualityMetrics {
        recall_at_5: found_at_5 as f64 / relevant_total,
        recall_at_10: relevant_at(TOP_K) as f64 / relevant_total,
        precision_at_5: found_at_5 as f64 / 5.0,
        precision_at_10: relevant_at(TOP_K) as f64 / TOP_K as f64,
        mrr_at_10,
        ndcg_at_10: if ideal_dcg == 0.0 {
            0.0
        } else {
            actual_dcg / ideal_dcg
        },
        success_at_5: if found_at_5 > 0 { 1.0 } else { 0.0 },
    }
}

fn unique_paths(ranked_paths: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ranked_paths
        .iter()
        .filter(|path| seen.insert(path.as_str()))
        .take(limit)
        .cloned()
        .collect()
}

fn dcg(grades: impl Iterator<Item = u8>) -> f64 {
    grades
        .enumerate()
        .map(|(rank, grade)| {
            let gain = 2_f64.powi(i32::from(grade)) - 1.0;
            gain / ((rank as f64 + 2.0).log2())
        })
        .sum()
}

fn citation_is_valid(corpus_dir: &Path, file_path: &str, range: &str) -> Result<bool> {
    let path = Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Ok(false);
    }
    let file_path = corpus_dir.join(path);
    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("search result cited a missing file {}", file_path.display()))?;
    let line_count = content.lines().count() as u32;
    let Some((start, end)) = range.split_once('-') else {
        return Ok(false);
    };
    let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>()) else {
        return Ok(false);
    };
    Ok(start > 0 && end >= start && end <= line_count)
}

fn in_memory_database_bytes(conn: &rusqlite::Connection) -> Result<u64> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(page_count.max(0) as u64 * page_size.max(0) as u64)
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f64 * percentile).ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn duration_cmp(left: &Duration, right: &Duration) -> std::cmp::Ordering {
    left.cmp(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graded_metrics_reward_relevant_documents_ranked_first() {
        let judgments = BTreeMap::from([
            ("best.rs".to_string(), 3),
            ("also-relevant.py".to_string(), 1),
        ]);
        let best_first = measure_query(
            &[
                "best.rs".to_string(),
                "noise.go".to_string(),
                "also-relevant.py".to_string(),
            ],
            &judgments,
        );
        let relevant_lower = measure_query(
            &[
                "noise.go".to_string(),
                "also-relevant.py".to_string(),
                "best.rs".to_string(),
            ],
            &judgments,
        );
        assert_eq!(best_first.recall_at_10, 1.0);
        assert_eq!(best_first.success_at_5, 1.0);
        assert!(best_first.mrr_at_10 > relevant_lower.mrr_at_10);
        assert!(best_first.ndcg_at_10 > relevant_lower.ndcg_at_10);
    }

    #[test]
    fn file_level_metrics_count_a_path_only_once_when_it_has_multiple_chunks() {
        let judgments = BTreeMap::from([("same.rs".to_string(), 3)]);
        let metrics = measure_query(
            &[
                "same.rs".to_string(),
                "same.rs".to_string(),
                "noise.py".to_string(),
            ],
            &judgments,
        );
        assert_eq!(metrics.recall_at_10, 1.0);
        assert_eq!(metrics.ndcg_at_10, 1.0);
    }

    #[test]
    fn checked_in_judged_corpus_has_complete_query_relevance() {
        let eval_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/retrieval");
        let queries = read_jsonl::<EvalQuery>(&eval_dir.join("queries.jsonl")).unwrap();
        let qrels = read_qrels(&eval_dir.join("qrels.jsonl")).unwrap();
        validate_qrels(&queries, &qrels, &eval_dir.join("corpus")).unwrap();
        let languages = queries
            .iter()
            .map(|query| query.language.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert!(queries.len() >= 30);
        assert!(
            languages.len() >= 17,
            "the judged set should span languages"
        );
        for query in queries {
            assert!(
                qrels
                    .get(&query.query_id)
                    .is_some_and(|docs| !docs.is_empty()),
                "query {} needs at least one judged relevant document",
                query.query_id
            );
        }
    }

    #[test]
    fn qrels_reject_unknown_query_ids_and_duplicate_paths() {
        let query = EvalQuery {
            query_id: "known".to_string(),
            text: "find it".to_string(),
            category: "intent".to_string(),
            language: "rust".to_string(),
            filter: None,
            expected_language: None,
        };
        let unknown = HashMap::from([(
            "unknown".to_string(),
            BTreeMap::from([("src/lib.rs".to_string(), 1)]),
        )]);
        let corpus = tempfile::tempdir().unwrap();
        fs::create_dir_all(corpus.path().join("src")).unwrap();
        fs::write(corpus.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
        assert!(validate_qrels(&[query.clone()], &unknown, corpus.path()).is_err());
        let missing = HashMap::new();
        assert!(validate_qrels(&[query], &missing, corpus.path()).is_err());

        let dir = tempfile::tempdir().unwrap();
        let qrels_path = dir.path().join("qrels.jsonl");
        fs::write(
            &qrels_path,
            "{\"query_id\":\"known\",\"path\":\"src/lib.rs\",\"grade\":1}\n{\"query_id\":\"known\",\"path\":\"src/lib.rs\",\"grade\":2}\n",
        )
        .unwrap();
        assert!(read_qrels(&qrels_path).is_err());
    }

    #[test]
    fn qrels_reject_paths_that_escape_the_corpus_root() {
        let corpus = tempfile::tempdir().unwrap();
        let query = EvalQuery {
            query_id: "known".to_string(),
            text: "find it".to_string(),
            category: "intent".to_string(),
            language: "rust".to_string(),
            filter: None,
            expected_language: None,
        };
        let qrels = HashMap::from([(
            "known".to_string(),
            BTreeMap::from([("../secret.rs".to_string(), 1)]),
        )]);

        assert!(validate_qrels(&[query], &qrels, corpus.path()).is_err());
    }
}
