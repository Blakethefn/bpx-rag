use super::super::search::QueryScores;
use super::super::*;
use super::CitationTally;
use crate::knowledge::query::{KnowledgeSearchResult, LayerReport};
use anyhow::Result;
use std::path::Path;

pub(super) fn build_case(
    query: &EvalQuery,
    scores: QueryScores,
    unanswerable_hybrid: Option<UnanswerableMetrics>,
    deep_paths: Vec<String>,
    layers: Vec<LayerReport>,
) -> EvalCaseReport {
    let is_answerable = query.answerability == QueryAnswerability::Answerable;
    EvalCaseReport {
        query_id: query.query_id.clone(),
        category: query.category.clone(),
        language: query.language.clone(),
        answerability: query.answerability,
        answerability_evidence: query.answerability_evidence.clone(),
        hybrid: is_answerable.then_some(scores.hybrid),
        lexical_only: is_answerable.then_some(scores.lexical),
        vector_only: is_answerable.then_some(scores.vector),
        unanswerable_hybrid,
        distinct_files_at_10: unique_paths(&scores.hybrid_paths, TOP_K).len(),
        top_paths: unique_paths(&scores.hybrid_paths, TOP_K),
        lexical_top_paths: unique_paths(&scores.lexical_paths, TOP_K),
        vector_top_paths: unique_paths(&scores.vector_paths, TOP_K),
        deep_paths,
        layers,
    }
}

pub(super) fn count_filter_leaks(
    results: &[KnowledgeSearchResult],
    expected_language: Option<&str>,
) -> usize {
    let Some(expected_language) = expected_language else {
        return 0;
    };
    results
        .iter()
        .filter(|result| {
            let frontmatter_language = result.metadata["frontmatter"]["language"].as_str();
            !crate::language::matches_language(
                expected_language,
                Path::new(&result.file_path),
                frontmatter_language,
            )
        })
        .count()
}

pub(super) fn record_citations(
    corpus_dir: &Path,
    query_id: &str,
    results: &[KnowledgeSearchResult],
    citations: &mut CitationTally,
) -> Result<()> {
    for result in results {
        citations.checked += 1;
        let (citation_valid, content_matches) = citation_quality(
            corpus_dir,
            &result.file_path,
            &result.lines,
            &result.snippet,
        )?;
        let issue = || CitationIssue {
            query_id: query_id.to_string(),
            path: result.file_path.clone(),
            lines: result.lines.clone(),
        };
        if citation_valid {
            citations.valid += 1;
        } else {
            citations.invalid.push(issue());
        }
        if content_matches {
            citations.content_matches += 1;
        } else {
            citations.content_mismatches.push(issue());
        }
    }
    Ok(())
}
