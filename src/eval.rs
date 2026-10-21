use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::embedder::{self, Embed, Embedder};
use crate::knowledge;
use crate::store::search::SearchWeights;

const TOP_K: usize = 10;

fn sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read evaluation file {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn corpus_manifest_sha256(corpus_root: &Path) -> Result<String> {
    let mut files = crate::knowledge::sync::walk_source_dir(corpus_root, None)?
        .into_iter()
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut manifest = Sha256::new();
    for (relative_path, absolute_path) in files {
        let content = fs::read(&absolute_path)
            .with_context(|| format!("failed to fingerprint {}", absolute_path.display()))?;
        manifest.update((relative_path.len() as u64).to_be_bytes());
        manifest.update(relative_path.as_bytes());
        manifest.update(Sha256::digest(content));
    }
    Ok(format!("{:x}", manifest.finalize()))
}

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
    #[serde(default)]
    judgment_source: Option<String>,
    #[serde(default)]
    answerability: QueryAnswerability,
    #[serde(default)]
    answerability_evidence: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswerability {
    #[default]
    Answerable,
    UnanswerableInCorpus,
}

#[derive(Debug, Deserialize, Serialize)]
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
    pub dataset_name: String,
    pub corpus_root_name: String,
    pub embedding_dimension: usize,
    pub batch_size: usize,
    pub intra_threads: usize,
    pub candidate_oversample_factor: u32,
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
    pub unanswerable_hybrid: UnanswerableMetrics,
    pub unanswerable_lexical_only: UnanswerableMetrics,
    pub unanswerable_vector_only: UnanswerableMetrics,
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

#[derive(Debug, Serialize)]
pub struct EmbeddingBenchmarkReport {
    pub model_name: String,
    pub model_id: String,
    pub corpus_root_name: String,
    pub host_threads: usize,
    pub files_scanned: usize,
    pub available_chunks: usize,
    pub measured_chunks: usize,
    pub warmup_chunks: usize,
    pub repetitions: usize,
    pub runs: Vec<EmbeddingBenchmarkRun>,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingBenchmarkRun {
    pub intra_threads: usize,
    pub batch_size: usize,
    pub model_load_ms: f64,
    pub warmup_ms: f64,
    pub embedding_ms_samples: Vec<f64>,
    pub embedding_ms: f64,
    pub chunks_per_second: f64,
    pub milliseconds_per_chunk: f64,
}

const JUDGMENT_POOL_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolArtifact {
    pub schema_version: u8,
    pub seed: u64,
    pub candidate_depth_per_retriever: usize,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
    pub corpus_file_count: usize,
    pub chunk_count: usize,
    pub grade_guide: Vec<GradeGuideEntry>,
    pub candidates: Vec<JudgmentPoolCandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolCandidate {
    pub blinded_query_id: String,
    pub query: String,
    pub candidate_id: String,
    pub path: String,
    pub evidence: Vec<JudgmentPoolEvidence>,
    pub grade: Option<u8>,
    pub reviewer_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GradeGuideEntry {
    pub grade: u8,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentPoolEvidence {
    pub title: String,
    pub lines: String,
    pub snippet: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolKey {
    pub schema_version: u8,
    pub seed: u64,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
    pub model_id: String,
    pub candidate_depth_per_retriever: usize,
    pub candidates: Vec<JudgmentPoolKeyEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolKeyEntry {
    pub candidate_id: String,
    pub blinded_query_id: String,
    pub query_id: String,
    pub path: String,
    pub retrieved_by: PoolRetrieverRanks,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolRetrieverRanks {
    pub hybrid: Option<usize>,
    pub lexical: Option<usize>,
    pub vector: Option<usize>,
}

#[derive(Default)]
struct PoolCandidateAccumulator {
    evidence: BTreeMap<String, JudgmentPoolEvidence>,
    retrieved_by: PoolRetrieverRanks,
}

#[derive(Debug, Serialize)]
pub struct JudgmentPoolGenerationReport {
    pub candidate_count: usize,
    pub query_count: usize,
    pub corpus_file_count: usize,
    pub chunk_count: usize,
    pub pool_output: String,
    pub key_output: String,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
}

/// Compare cached ONNX model throughput over a deterministic sample produced by
/// the same ignore rules and chunker used by persistent knowledge indexing.
pub fn run_embedding_benchmark(
    corpus_root: &Path,
    thread_counts: &[usize],
    batch_sizes: &[usize],
    repetitions: usize,
    max_chunks: usize,
) -> Result<EmbeddingBenchmarkReport> {
    let host_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 256);
    let thread_counts = if thread_counts.is_empty() {
        default_thread_sweep(host_threads)
    } else {
        thread_counts.to_vec()
    };
    if !corpus_root.is_dir() {
        anyhow::bail!(
            "benchmark corpus is not a directory: {}",
            corpus_root.display()
        );
    }
    if thread_counts.iter().any(|value| !(1..=256).contains(value))
        || batch_sizes.is_empty()
        || batch_sizes.iter().any(|value| !(1..=256).contains(value))
    {
        anyhow::bail!("thread and batch-size values must be between 1 and 256");
    }
    if max_chunks == 0 || max_chunks > 100_000 {
        anyhow::bail!("max-chunks must be between 1 and 100000");
    }
    if !(1..=10).contains(&repetitions) {
        anyhow::bail!("repetitions must be between 1 and 10");
    }
    if thread_counts.len().saturating_mul(batch_sizes.len()) > 64 {
        anyhow::bail!("embedding benchmark is limited to 64 thread/batch combinations");
    }

    let config = Config::load()?;
    let model_dir = crate::rag_app::model_directory(&config);
    if !embedder::model_files_present(&model_dir, &config.embeddings.model) {
        anyhow::bail!(
            "embedding benchmark requires the configured model to be cached; run `bpx-rag models download` first ({})",
            model_dir.display()
        );
    }

    let mut files = crate::knowledge::sync::walk_source_dir(corpus_root, None)?
        .into_iter()
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let files_scanned = files.len();
    let mut available_chunks = 0usize;
    let mut file_hashes = Vec::with_capacity(files.len());
    for (_, path) in &files {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read benchmark file {}", path.display()))?;
        available_chunks += crate::store::chunker::chunk_content(&content).len();
        file_hashes.push(sha256(&content));
    }
    if available_chunks == 0 {
        anyhow::bail!("benchmark corpus contains no indexable chunks");
    }
    let measured_chunks = available_chunks.min(max_chunks);
    let sample_indices = if measured_chunks == 1 {
        vec![available_chunks / 2]
    } else {
        (0..measured_chunks)
            .map(|position| {
                (position as u128 * (available_chunks - 1) as u128 / (measured_chunks - 1) as u128)
                    as usize
            })
            .collect::<Vec<_>>()
    };
    let mut texts = Vec::with_capacity(measured_chunks);
    let mut next_sample = 0;
    let mut chunk_index = 0;
    for ((_, path), expected_hash) in files.iter().zip(&file_hashes) {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read benchmark file {}", path.display()))?;
        if sha256(&content) != *expected_hash {
            anyhow::bail!(
                "benchmark source changed while chunks were being sampled: {}; retry evaluation",
                path.display()
            );
        }
        for chunk in crate::store::chunker::chunk_content(&content) {
            if sample_indices.get(next_sample) == Some(&chunk_index) {
                texts.push(format!("{} {}", chunk.title, chunk.content));
                next_sample += 1;
            }
            chunk_index += 1;
        }
    }
    if texts.len() != measured_chunks || chunk_index != available_chunks {
        anyhow::bail!("benchmark corpus changed while chunks were being sampled; retry evaluation");
    }

    let model_id = embedder::canonical_model_id(&config.embeddings.model)?;
    let warmup_chunks = texts.len().min(8);
    let refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
    let mut runs = Vec::with_capacity(thread_counts.len() * batch_sizes.len());

    for intra_threads in thread_counts {
        for &batch_size in batch_sizes {
            let load_start = Instant::now();
            let model = Embedder::load_cached(
                &model_dir,
                &config.embeddings.model,
                batch_size,
                intra_threads,
            )?;
            let model_load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

            let warmup_start = Instant::now();
            let warmup = model.embed_batch(&refs[..warmup_chunks])?;
            let warmup_ms = warmup_start.elapsed().as_secs_f64() * 1000.0;
            if warmup.len() != warmup_chunks {
                anyhow::bail!("embedding provider returned an incomplete warmup result");
            }

            let mut embedding_ms_samples = Vec::with_capacity(repetitions);
            for _ in 0..repetitions {
                let embedding_start = Instant::now();
                let vectors = model.embed_batch(&refs)?;
                let embedding_ms = embedding_start.elapsed().as_secs_f64() * 1000.0;
                if vectors.len() != refs.len() || vectors.iter().any(|v| v.len() != model.dim()) {
                    anyhow::bail!("embedding provider returned invalid vector shapes");
                }
                embedding_ms_samples.push(embedding_ms);
            }
            let mut sorted_samples = embedding_ms_samples.clone();
            sorted_samples.sort_by(f64::total_cmp);
            let embedding_ms = if sorted_samples.len() % 2 == 0 {
                let middle = sorted_samples.len() / 2;
                (sorted_samples[middle - 1] + sorted_samples[middle]) / 2.0
            } else {
                sorted_samples[sorted_samples.len() / 2]
            };
            let elapsed_seconds = (embedding_ms / 1000.0).max(f64::EPSILON);
            runs.push(EmbeddingBenchmarkRun {
                intra_threads,
                batch_size,
                model_load_ms,
                warmup_ms,
                embedding_ms_samples,
                embedding_ms,
                chunks_per_second: refs.len() as f64 / elapsed_seconds,
                milliseconds_per_chunk: embedding_ms / refs.len() as f64,
            });
        }
    }

    Ok(EmbeddingBenchmarkReport {
        model_name: config.embeddings.model,
        model_id,
        corpus_root_name: corpus_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("corpus")
            .to_string(),
        host_threads,
        files_scanned,
        available_chunks,
        measured_chunks: texts.len(),
        warmup_chunks,
        repetitions,
        runs,
    })
}

/// Build a blinded annotation pool by merging file candidates from the hybrid,
/// lexical, and vector search channels. The rank-reveal key is written
/// separately from the annotator artifact.
pub fn generate_judgment_pool(
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
    pool_output: &Path,
    key_output: &Path,
    depth: usize,
    seed: u64,
) -> Result<JudgmentPoolGenerationReport> {
    if !(1..=100).contains(&depth) {
        anyhow::bail!("pool depth must be between 1 and 100");
    }
    ensure_new_distinct_outputs(pool_output, key_output)?;

    let config = Config::load()?;
    let model_dir = crate::rag_app::model_directory(&config);
    if !embedder::model_files_present(&model_dir, &config.embeddings.model) {
        anyhow::bail!(
            "judgment pooling requires the configured model to be cached; run `bpx-rag models download` first ({})",
            model_dir.display()
        );
    }
    let model = Embedder::load_cached(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
    )?;
    let model_id = model.model_id().to_string();

    let data_dir = dataset.map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/retrieval"),
        Path::to_path_buf,
    );
    let corpus_dir = corpus_root.map_or_else(|| data_dir.join("corpus"), Path::to_path_buf);
    if !data_dir.is_dir() {
        anyhow::bail!(
            "judgment dataset directory does not exist: {}",
            data_dir.display()
        );
    }
    if !corpus_dir.is_dir() {
        anyhow::bail!(
            "judgment corpus directory does not exist: {}",
            corpus_dir.display()
        );
    }
    let query_path = data_dir.join("queries.jsonl");
    let query_bytes = fs::read(&query_path)
        .with_context(|| format!("failed to read evaluation queries {}", query_path.display()))?;
    let dataset_queries_sha256 = format!("{:x}", Sha256::digest(&query_bytes));
    let query_contents = String::from_utf8(query_bytes)
        .with_context(|| format!("evaluation queries are not UTF-8: {}", query_path.display()))?;
    let queries = parse_jsonl::<EvalQuery>(&query_contents, &query_path)?;
    validate_queries(&queries)?;
    if queries.is_empty() {
        anyhow::bail!("judgment query set is empty");
    }
    let corpus_before = corpus_manifest_sha256(&corpus_dir)?;

    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(model));
    store.add_source(
        "judgment-pool",
        &corpus_dir.to_string_lossy(),
        None,
        &["frontmatter".into()],
    )?;
    let source = store
        .get_source("judgment-pool")?
        .ok_or_else(|| anyhow!("could not load judgment-pool source"))?;
    let enrichment =
        knowledge::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrichment_ref = enrichment
        .as_ref()
        .map(|function| function.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    let sync = knowledge::sync::sync_source(&store, &source, enrichment_ref)?;
    let corpus_after = corpus_manifest_sha256(&corpus_dir)?;
    if corpus_before != corpus_after {
        anyhow::bail!("judgment corpus changed during indexing; retry pool generation");
    }

    let mut pool_candidates = Vec::new();
    let mut key_candidates = Vec::new();
    for query in &queries {
        let blinded_query_id = opaque_id(seed, "query", &query.query_id);
        let mut candidates: BTreeMap<String, PoolCandidateAccumulator> = BTreeMap::new();
        let channels = [
            (
                "hybrid",
                SearchWeights {
                    keyword_weight: 1.0,
                    vector_weight: 1.0,
                },
            ),
            (
                "lexical",
                SearchWeights {
                    keyword_weight: 1.0,
                    vector_weight: 0.0,
                },
            ),
            (
                "vector",
                SearchWeights {
                    keyword_weight: 0.0,
                    vector_weight: 1.0,
                },
            ),
        ];
        for (channel, weights) in channels {
            let results = knowledge::search::knowledge_search(
                &store,
                &query.text,
                query.filter.as_deref(),
                Some("judgment-pool"),
                depth as u32,
                &weights,
                1200,
            )?;
            for (rank, result) in results.into_iter().enumerate() {
                let candidate = candidates.entry(result.file_path.clone()).or_default();
                let evidence_key = format!(
                    "{}:{}:{}",
                    result.lines,
                    result.title,
                    sha256(&result.snippet)
                );
                candidate
                    .evidence
                    .entry(evidence_key)
                    .or_insert(JudgmentPoolEvidence {
                        title: result.title,
                        lines: result.lines,
                        snippet: result.snippet,
                    });
                match channel {
                    "hybrid" => {
                        candidate.retrieved_by.hybrid = Some(
                            candidate
                                .retrieved_by
                                .hybrid
                                .map_or(rank + 1, |old| old.min(rank + 1)),
                        );
                    }
                    "lexical" => {
                        candidate.retrieved_by.lexical = Some(
                            candidate
                                .retrieved_by
                                .lexical
                                .map_or(rank + 1, |old| old.min(rank + 1)),
                        );
                    }
                    "vector" => {
                        candidate.retrieved_by.vector = Some(
                            candidate
                                .retrieved_by
                                .vector
                                .map_or(rank + 1, |old| old.min(rank + 1)),
                        );
                    }
                    _ => anyhow::bail!("unexpected judgment-pool retrieval channel"),
                }
            }
        }

        if query.answerability == QueryAnswerability::UnanswerableInCorpus {
            for (path, evidence) in indexed_corpus_evidence(&store, source.id)? {
                let candidate = candidates.entry(path.clone()).or_default();
                for evidence in evidence {
                    let evidence_key = format!(
                        "{}:{}:{}",
                        evidence.lines,
                        evidence.title,
                        sha256(&evidence.snippet)
                    );
                    candidate.evidence.entry(evidence_key).or_insert(evidence);
                }
            }
        }

        for (path, candidate) in candidates {
            let candidate_id =
                opaque_id(seed, "candidate", &format!("{}:{path}", blinded_query_id));
            pool_candidates.push(JudgmentPoolCandidate {
                blinded_query_id: blinded_query_id.clone(),
                query: query.text.clone(),
                candidate_id: candidate_id.clone(),
                path: path.clone(),
                evidence: candidate.evidence.into_values().collect(),
                grade: None,
                reviewer_notes: None,
            });
            key_candidates.push(JudgmentPoolKeyEntry {
                candidate_id,
                blinded_query_id: blinded_query_id.clone(),
                query_id: query.query_id.clone(),
                path,
                retrieved_by: candidate.retrieved_by,
            });
        }
    }

    pool_candidates.sort_by(|left, right| {
        (&left.blinded_query_id, &left.candidate_id)
            .cmp(&(&right.blinded_query_id, &right.candidate_id))
    });
    key_candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    if sha256_file(&query_path)? != dataset_queries_sha256 {
        anyhow::bail!("evaluation queries changed during judgment-pool generation; retry");
    }
    if corpus_manifest_sha256(&corpus_dir)? != corpus_after {
        anyhow::bail!("judgment corpus changed during candidate retrieval; retry pool generation");
    }
    let corpus_manifest_sha256 = corpus_after;
    let pool_artifact = JudgmentPoolArtifact {
        schema_version: JUDGMENT_POOL_SCHEMA_VERSION,
        seed,
        candidate_depth_per_retriever: depth,
        dataset_queries_sha256: dataset_queries_sha256.clone(),
        corpus_manifest_sha256: corpus_manifest_sha256.clone(),
        corpus_file_count: sync.files_scanned,
        chunk_count: sync.chunks_total,
        grade_guide: vec![
            GradeGuideEntry {
                grade: 0,
                meaning: "Not relevant to answering this query".into(),
            },
            GradeGuideEntry {
                grade: 1,
                meaning: "Related, but provides little direct help".into(),
            },
            GradeGuideEntry {
                grade: 2,
                meaning: "Useful supporting evidence".into(),
            },
            GradeGuideEntry {
                grade: 3,
                meaning: "Directly answers or is essential to answering the query".into(),
            },
        ],
        candidates: pool_candidates,
    };
    let key_artifact = JudgmentPoolKey {
        schema_version: JUDGMENT_POOL_SCHEMA_VERSION,
        seed,
        dataset_queries_sha256: dataset_queries_sha256.clone(),
        corpus_manifest_sha256: corpus_manifest_sha256.clone(),
        model_id,
        candidate_depth_per_retriever: depth,
        candidates: key_candidates,
    };
    write_new_json(pool_output, &pool_artifact)?;
    write_new_json(key_output, &key_artifact)?;

    Ok(JudgmentPoolGenerationReport {
        candidate_count: pool_artifact.candidates.len(),
        query_count: queries.len(),
        corpus_file_count: sync.files_scanned,
        chunk_count: sync.chunks_total,
        pool_output: pool_output.display().to_string(),
        key_output: key_output.display().to_string(),
        dataset_queries_sha256,
        corpus_manifest_sha256,
    })
}

fn indexed_corpus_evidence(
    store: &knowledge::KnowledgeStore,
    source_id: i64,
) -> Result<BTreeMap<String, Vec<JudgmentPoolEvidence>>> {
    let mut statement = store.conn().prepare(
        "SELECT kf.rel_path, kc.title, kc.content, kcm.line_start, kcm.line_end
         FROM knowledge_files kf
         LEFT JOIN knowledge_chunk_meta kcm ON kcm.file_id = kf.id
         LEFT JOIN knowledge_chunks kc ON kc.rowid = kcm.chunk_rowid
         WHERE kf.source_id = ?1
         ORDER BY kf.rel_path, kcm.line_start, kcm.line_end, kcm.chunk_rowid",
    )?;
    let mut files: BTreeMap<String, Vec<JudgmentPoolEvidence>> = BTreeMap::new();
    let rows = statement.query_map([source_id], |row| {
        let path: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let snippet: Option<String> = row.get(2)?;
        let line_start: Option<i64> = row.get(3)?;
        let line_end: Option<i64> = row.get(4)?;
        Ok((path, title, snippet, line_start, line_end))
    })?;
    for row in rows {
        let (path, title, snippet, line_start, line_end) = row?;
        let evidence = files.entry(path).or_default();
        if let (Some(title), Some(snippet), Some(line_start), Some(line_end)) =
            (title, snippet, line_start, line_end)
        {
            evidence.push(JudgmentPoolEvidence {
                title,
                lines: format!("{line_start}-{line_end}"),
                snippet,
            });
        }
    }
    Ok(files)
}

/// Import completed human grades from a pool and its separate reveal key.
/// Unreviewed candidates stay out of qrels, while explicit grade zero remains
/// a judged nonrelevant document.
pub fn import_judgment_pool(
    pool_path: &Path,
    key_path: &Path,
    dataset: &Path,
    corpus_root: &Path,
    output: &Path,
) -> Result<usize> {
    ensure_new_output(output)?;
    let output_identity = normalized_output_target(output)?;
    if output_identity == normalized_output_target(pool_path)?
        || output_identity == normalized_output_target(key_path)?
    {
        anyhow::bail!("qrels output must differ from the pool and reveal-key files");
    }
    let pool: JudgmentPoolArtifact = read_json_file(pool_path)?;
    let key: JudgmentPoolKey = read_json_file(key_path)?;
    if pool.schema_version != JUDGMENT_POOL_SCHEMA_VERSION
        || key.schema_version != JUDGMENT_POOL_SCHEMA_VERSION
    {
        anyhow::bail!("unsupported judgment-pool schema version");
    }
    if pool.seed != key.seed
        || pool.dataset_queries_sha256 != key.dataset_queries_sha256
        || pool.corpus_manifest_sha256 != key.corpus_manifest_sha256
        || pool.candidate_depth_per_retriever != key.candidate_depth_per_retriever
    {
        anyhow::bail!("judgment pool and reveal key do not match");
    }
    let query_path = dataset.join("queries.jsonl");
    let queries = read_jsonl::<EvalQuery>(&query_path)?;
    let query_ids = validate_queries(&queries)?;
    let queries_by_id = queries
        .iter()
        .map(|query| (query.query_id.as_str(), query))
        .collect::<HashMap<_, _>>();
    if sha256_file(&query_path)? != pool.dataset_queries_sha256 {
        anyhow::bail!("dataset queries changed since the judgment pool was created");
    }
    if corpus_manifest_sha256(corpus_root)? != pool.corpus_manifest_sha256 {
        anyhow::bail!("corpus changed since the judgment pool was created");
    }

    let mut key_by_candidate = HashMap::with_capacity(key.candidates.len());
    for entry in &key.candidates {
        if !query_ids.contains(&entry.query_id) {
            anyhow::bail!("reveal key contains unknown query ID {}", entry.query_id);
        }
        let expected_blinded_id = opaque_id(key.seed, "query", &entry.query_id);
        if entry.blinded_query_id != expected_blinded_id {
            anyhow::bail!("reveal key query ID does not match its blinded ID");
        }
        let expected_candidate_id = opaque_id(
            key.seed,
            "candidate",
            &format!("{}:{}", entry.blinded_query_id, entry.path),
        );
        if entry.candidate_id != expected_candidate_id {
            anyhow::bail!("reveal key candidate ID does not match its query and path");
        }
        if key_by_candidate
            .insert(entry.candidate_id.clone(), entry)
            .is_some()
        {
            anyhow::bail!(
                "duplicate candidate ID in reveal key: {}",
                entry.candidate_id
            );
        }
    }

    let mut seen_pool_ids = std::collections::HashSet::new();
    let mut seen_qrel_pairs = std::collections::HashSet::new();
    let mut qrels = Vec::new();
    let mut qrel_map: HashMap<String, BTreeMap<String, u8>> = HashMap::new();
    for candidate in &pool.candidates {
        if !seen_pool_ids.insert(candidate.candidate_id.as_str()) {
            anyhow::bail!(
                "duplicate candidate ID in annotation pool: {}",
                candidate.candidate_id
            );
        }
        let entry = key_by_candidate
            .get(&candidate.candidate_id)
            .ok_or_else(|| {
                anyhow!(
                    "candidate is missing from the reveal key: {}",
                    candidate.candidate_id
                )
            })?;
        if entry.blinded_query_id != candidate.blinded_query_id || entry.path != candidate.path {
            anyhow::bail!("annotation candidate does not match its reveal-key entry");
        }
        let source_query = queries_by_id
            .get(entry.query_id.as_str())
            .ok_or_else(|| anyhow!("reveal key references unknown query {}", entry.query_id))?;
        if candidate.query != source_query.text {
            anyhow::bail!("annotation candidate query text does not match the source dataset");
        }
        let Some(grade) = candidate.grade else {
            continue;
        };
        if grade > 3 {
            anyhow::bail!(
                "candidate grade must be between 0 and 3: {}",
                candidate.candidate_id
            );
        }
        if !seen_qrel_pairs.insert((entry.query_id.clone(), entry.path.clone())) {
            anyhow::bail!(
                "annotation pool contains duplicate judgments for {} and {}",
                entry.query_id,
                entry.path
            );
        }
        qrels.push(Qrel {
            query_id: entry.query_id.clone(),
            path: entry.path.clone(),
            grade,
        });
        qrel_map
            .entry(entry.query_id.clone())
            .or_default()
            .insert(entry.path.clone(), grade);
    }
    if seen_pool_ids.len() != key_by_candidate.len() {
        anyhow::bail!("reveal key contains candidates absent from the annotation pool");
    }
    if qrels.is_empty() {
        anyhow::bail!("annotation pool contains no completed grades");
    }
    validate_qrels(&queries, &qrel_map, corpus_root)?;
    qrels.sort_by(|left, right| (&left.query_id, &left.path).cmp(&(&right.query_id, &right.path)));
    write_new_jsonl(output, &qrels)?;
    Ok(qrels.len())
}

fn opaque_id(seed: u64, kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_be_bytes());
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{kind}-{}", &digest[..20])
}

fn normalized_output_target(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path must include a file name"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        anyhow::bail!("output directory does not exist: {}", parent.display());
    }
    Ok(parent.canonicalize()?.join(file_name))
}

fn ensure_new_output(path: &Path) -> Result<()> {
    let _ = normalized_output_target(path)?;
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing output {}", path.display());
    }
    Ok(())
}

fn ensure_new_distinct_outputs(first: &Path, second: &Path) -> Result<()> {
    ensure_new_output(first)?;
    ensure_new_output(second)?;
    if normalized_output_target(first)? == normalized_output_target(second)? {
        anyhow::bail!("pool and reveal-key outputs must be different files");
    }
    Ok(())
}

fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn write_new_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON file {}", path.display()))
}

fn default_thread_sweep(host_threads: usize) -> Vec<usize> {
    let candidates = [
        host_threads.div_ceil(3),
        host_threads.saturating_mul(2).div_ceil(3),
        host_threads,
    ];
    let mut values = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if values.last().copied() != Some(candidate) {
            values.push(candidate);
        }
    }
    values
}

#[derive(Clone, Copy, Default)]
struct MetricSums {
    recall_at_5: f64,
    recall_at_10: f64,
    precision_at_5: f64,
    precision_at_10: f64,
    judged_coverage_at_5: f64,
    judged_coverage_at_10: f64,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    success_at_5: f64,
    count: usize,
}

#[derive(Clone, Copy, Default)]
struct UnanswerableMetricSums {
    explicit_negative_share_at_10: f64,
    judged_coverage_at_10: f64,
    empty_result_rate: f64,
    count: usize,
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

fn measure_unanswerable_query(
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
        config.embeddings.intra_threads,
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
    let corpus_manifest_sha256 = corpus_manifest_sha256(&corpus_dir)?;
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
    let mut unanswerable_hybrid = UnanswerableMetricSums::default();
    let mut unanswerable_lexical = UnanswerableMetricSums::default();
    let mut unanswerable_vector = UnanswerableMetricSums::default();
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
        batch_size: config.embeddings.batch_size,
        intra_threads: config.embeddings.intra_threads,
        candidate_oversample_factor: knowledge::search::CANDIDATE_OVERSAMPLE_FACTOR,
        queries_sha256,
        qrels_sha256,
        corpus_manifest_sha256,
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
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read evaluation data {}", path.display()))?;
    parse_jsonl(&contents, path)
}

fn parse_jsonl<T: for<'de> Deserialize<'de>>(contents: &str, path: &Path) -> Result<Vec<T>> {
    contents
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

fn validate_qrels(
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
    let indexable_paths = crate::knowledge::sync::walk_source_dir(corpus_dir, None)?
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

fn validate_queries(queries: &[EvalQuery]) -> Result<std::collections::HashSet<String>> {
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

fn measure_query(ranked_paths: &[String], judgments: &BTreeMap<String, u8>) -> QualityMetrics {
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
            judgment_source: None,
            answerability: QueryAnswerability::Answerable,
            answerability_evidence: None,
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
            judgment_source: None,
            answerability: QueryAnswerability::Answerable,
            answerability_evidence: None,
        };
        let qrels = HashMap::from([(
            "known".to_string(),
            BTreeMap::from([("../secret.rs".to_string(), 1)]),
        )]);

        assert!(validate_qrels(&[query], &qrels, corpus.path()).is_err());
    }
}
