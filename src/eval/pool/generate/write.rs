use super::super::super::{corpus_manifest_sha256, sha256_file};
use super::super::output::write_new_json;
use super::super::{
    GradeGuideEntry, JudgmentPoolArtifact, JudgmentPoolCandidate, JudgmentPoolGenerationReport,
    JudgmentPoolKey, JudgmentPoolKeyEntry, JUDGMENT_POOL_SCHEMA_VERSION,
};
use super::load::LoadedPool;
use anyhow::Result;
use std::path::Path;

pub(super) fn write_outputs(
    pool_output: &Path,
    key_output: &Path,
    loaded: &LoadedPool,
    mut pool_candidates: Vec<JudgmentPoolCandidate>,
    mut key_candidates: Vec<JudgmentPoolKeyEntry>,
    depth: usize,
    seed: u64,
) -> Result<JudgmentPoolGenerationReport> {
    pool_candidates.sort_by(|left, right| {
        (&left.blinded_query_id, &left.candidate_id)
            .cmp(&(&right.blinded_query_id, &right.candidate_id))
    });
    key_candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    if sha256_file(&loaded.query_path)? != loaded.dataset_queries_sha256 {
        anyhow::bail!("evaluation queries changed during judgment-pool generation; retry");
    }
    if corpus_manifest_sha256(&loaded.corpus_dir)? != loaded.corpus_manifest_sha256 {
        anyhow::bail!("judgment corpus changed during candidate retrieval; retry pool generation");
    }
    let corpus_manifest_sha256 = loaded.corpus_manifest_sha256.clone();
    let pool_artifact = JudgmentPoolArtifact {
        schema_version: JUDGMENT_POOL_SCHEMA_VERSION,
        seed,
        candidate_depth_per_retriever: depth,
        dataset_queries_sha256: loaded.dataset_queries_sha256.clone(),
        corpus_manifest_sha256: corpus_manifest_sha256.clone(),
        corpus_file_count: loaded.files_scanned,
        chunk_count: loaded.chunks_total,
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
        dataset_queries_sha256: loaded.dataset_queries_sha256.clone(),
        corpus_manifest_sha256: corpus_manifest_sha256.clone(),
        model_id: loaded.model_id.clone(),
        candidate_depth_per_retriever: depth,
        candidates: key_candidates,
    };
    write_new_json(pool_output, &pool_artifact)?;
    write_new_json(key_output, &key_artifact)?;

    Ok(JudgmentPoolGenerationReport {
        candidate_count: pool_artifact.candidates.len(),
        query_count: loaded.queries.len(),
        corpus_file_count: loaded.files_scanned,
        chunk_count: loaded.chunks_total,
        pool_output: pool_output.display().to_string(),
        key_output: key_output.display().to_string(),
        dataset_queries_sha256: loaded.dataset_queries_sha256.clone(),
        corpus_manifest_sha256,
    })
}
