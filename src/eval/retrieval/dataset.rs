use super::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub(in crate::eval) struct EvalQuery {
    pub(in crate::eval) query_id: String,
    pub(in crate::eval) text: String,
    pub(in crate::eval) category: String,
    pub(in crate::eval) language: String,
    #[serde(default)]
    pub(in crate::eval) filter: Option<String>,
    #[serde(default)]
    pub(in crate::eval) expected_language: Option<String>,
    #[serde(default)]
    pub(in crate::eval) judgment_source: Option<String>,
    #[serde(default)]
    pub(in crate::eval) answerability: QueryAnswerability,
    #[serde(default)]
    pub(in crate::eval) answerability_evidence: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(in crate::eval) struct Qrel {
    pub(in crate::eval) query_id: String,
    pub(in crate::eval) path: String,
    pub(in crate::eval) grade: u8,
}

pub(in crate::eval) fn read_qrels(path: &Path) -> Result<HashMap<String, BTreeMap<String, u8>>> {
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

pub(in crate::eval) fn validate_qrels(
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

pub(in crate::eval) fn validate_queries(
    queries: &[EvalQuery],
) -> Result<std::collections::HashSet<String>> {
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
