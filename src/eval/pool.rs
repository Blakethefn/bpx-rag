use super::*;
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use crate::knowledge::query::SearchWeights;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) const JUDGMENT_POOL_SCHEMA_VERSION: u8 = 1;

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
pub(super) struct PoolCandidateAccumulator {
    pub(super) evidence: BTreeMap<String, JudgmentPoolEvidence>,
    pub(super) retrieved_by: PoolRetrieverRanks,
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
    let model_dir = crate::app::model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        anyhow::bail!(
            "judgment pooling requires the configured model to be cached; run `bpx-rag models download` first ({})",
            model_dir.display()
        );
    }
    let model = Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
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
        &[],
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
            let results = knowledge::query::knowledge_search(
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

pub(super) fn indexed_corpus_evidence(
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

pub(super) fn opaque_id(seed: u64, kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_be_bytes());
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{kind}-{}", &digest[..20])
}

pub(super) fn normalized_output_target(path: &Path) -> Result<PathBuf> {
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

pub(super) fn ensure_new_output(path: &Path) -> Result<()> {
    let _ = normalized_output_target(path)?;
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing output {}", path.display());
    }
    Ok(())
}

pub(super) fn ensure_new_distinct_outputs(first: &Path, second: &Path) -> Result<()> {
    ensure_new_output(first)?;
    ensure_new_output(second)?;
    if normalized_output_target(first)? == normalized_output_target(second)? {
        anyhow::bail!("pool and reveal-key outputs must be different files");
    }
    Ok(())
}

pub(super) fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
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

pub(super) fn write_new_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
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

pub(super) fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON file {}", path.display()))
}
