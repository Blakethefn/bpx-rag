use super::*;
use crate::knowledge::query::SearchWeights;
use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub(super) mod index;
pub(super) mod model;

pub(super) fn validate_run_options(
    repetitions: usize,
    hybrid_weights: &SearchWeights,
) -> Result<()> {
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

pub(super) struct EvalPaths {
    pub(super) data_dir: PathBuf,
    pub(super) corpus_dir: PathBuf,
    pub(super) query_path: PathBuf,
    pub(super) qrels_path: PathBuf,
}

pub(super) fn resolve_eval_paths(
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
) -> Result<EvalPaths> {
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

pub(super) struct EvalData {
    pub(super) queries: Vec<EvalQuery>,
    pub(super) qrels: HashMap<String, BTreeMap<String, u8>>,
    pub(super) queries_sha256: String,
    pub(super) qrels_sha256: String,
    pub(super) corpus_manifest_sha256: String,
    pub(super) positive_judgment_count: usize,
    pub(super) explicit_negative_judgment_count: usize,
    pub(super) judgment_sources: Vec<String>,
}

pub(super) fn load_eval_data(paths: &EvalPaths) -> Result<EvalData> {
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
