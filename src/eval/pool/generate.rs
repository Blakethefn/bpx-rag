use super::super::{
    corpus_manifest_sha256, parse_jsonl, sha256, sha256_file, validate_queries, EvalQuery,
    QueryAnswerability,
};
use super::{
    ensure_new_distinct_outputs, opaque_id, write_new_json, GradeGuideEntry, JudgmentPoolArtifact,
    JudgmentPoolCandidate, JudgmentPoolEvidence, JudgmentPoolGenerationReport, JudgmentPoolKey,
    JudgmentPoolKeyEntry, PoolCandidateAccumulator, PoolChannel, JUDGMENT_POOL_SCHEMA_VERSION,
};
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        for channel in PoolChannel::ALL {
            let weights = channel.weights();
            let results = knowledge::query::knowledge_search(
                &store,
                &query.text,
                query.filter.as_deref(),
                Some("judgment-pool"),
                depth as u32,
                &weights,
                1200,
            )?;
            for (offset, result) in results.into_iter().enumerate() {
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
                channel.record_rank(&mut candidate.retrieved_by, offset + 1);
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

pub(in crate::eval) fn indexed_corpus_evidence(
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
