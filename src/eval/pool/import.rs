use super::super::{
    corpus_manifest_sha256, read_jsonl, sha256_file, validate_qrels, validate_queries, EvalQuery,
    Qrel,
};
use super::{
    ensure_new_output, normalized_output_target, opaque_id, read_json_file, write_new_jsonl,
    JudgmentPoolArtifact, JudgmentPoolKey, JUDGMENT_POOL_SCHEMA_VERSION,
};
use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

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
