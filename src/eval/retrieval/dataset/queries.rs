use super::super::QueryAnswerability;
use super::EvalQuery;
use anyhow::Result;

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
