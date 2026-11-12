use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use super::*;
use crate::config::Config;
use crate::embed;
use crate::knowledge::query::{LayerPolicy, SearchWeights};

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
    assert!(validate_qrels(std::slice::from_ref(&query), &unknown, corpus.path()).is_err());
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

#[test]
fn retrieval_report_pins_fixture_counts_and_case_order() {
    let config = Config::load().expect("config loads");
    let model_dir = crate::app::model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        eprintln!(
            "skipping retrieval fixture: embedding model is not cached at {}",
            model_dir.display()
        );
        return;
    }
    if matches!(
        config.embeddings.device,
        crate::config::EmbeddingDevice::Cuda
    ) && !cfg!(feature = "gpu-cuda")
    {
        eprintln!("skipping retrieval fixture: configured device is CUDA but gpu-cuda is off");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("dataset");
    let corpus_dir = root.path().join("corpus");
    fs::create_dir_all(corpus_dir.join("src")).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        corpus_dir.join("src/alpha.rs"),
        "pub fn alpha_parser(input: &str) -> usize {\n    input.len()\n}\n",
    )
    .unwrap();
    fs::write(
        corpus_dir.join("src/beta.rs"),
        "pub fn beta_renderer(width: usize) -> usize {\n    width * 2\n}\n",
    )
    .unwrap();
    fs::write(
        data_dir.join("queries.jsonl"),
        concat!(
            "{\"query_id\":\"q-alpha\",\"text\":\"alpha parser\",\"category\":\"intent\",\"language\":\"rust\",\"expected_language\":\"rust\",\"judgment_source\":\" fixture \"}\n",
            "{\"query_id\":\"q-none\",\"text\":\"database migration\",\"category\":\"absent\",\"language\":\"rust\",\"answerability\":\"unanswerable_in_corpus\",\"answerability_evidence\":\"both files reviewed\"}\n",
        ),
    )
    .unwrap();
    fs::write(
        data_dir.join("qrels.jsonl"),
        concat!(
            "{\"query_id\":\"q-alpha\",\"path\":\"src/alpha.rs\",\"grade\":3}\n",
            "{\"query_id\":\"q-none\",\"path\":\"src/alpha.rs\",\"grade\":0}\n",
            "{\"query_id\":\"q-none\",\"path\":\"src/beta.rs\",\"grade\":0}\n",
        ),
    )
    .unwrap();

    let report = run_retrieval_evaluation(
        1,
        Some(&data_dir),
        Some(&corpus_dir),
        SearchWeights::default(),
        LayerPolicy::default(),
    )
    .expect("fixture evaluation runs");

    assert_eq!(report.query_count, 2);
    assert_eq!(report.answerable_query_count, 1);
    assert_eq!(report.unanswerable_query_count, 1);
    assert_eq!(report.positive_judgment_count, 1);
    assert_eq!(report.explicit_negative_judgment_count, 2);
    assert_eq!(report.judgment_sources, vec!["fixture".to_string()]);
    assert_eq!(report.corpus_root_name, "corpus");
    assert_eq!(report.corpus_file_count, 2);
    assert_eq!(report.repetitions, 1);
    assert_eq!(report.queries_with_relation_neighbors, 0);
    assert_eq!(report.filter_leaks, 0);
    assert_eq!(report.citation_validity, 1.0);
    assert_eq!(report.citation_content_validity, Some(1.0));
    assert!(report.citation_content_mismatches.is_empty());
    assert!(report.invalid_citations.is_empty());
    assert!(report.citation_count > 0);
    assert_eq!(report.overall.recall_at_10, 1.0);
    assert_eq!(report.overall.success_at_5, 1.0);
    assert_eq!(report.unanswerable_hybrid.query_count, 1);
    assert_eq!(report.by_category.keys().collect::<Vec<_>>(), ["intent"]);
    assert_eq!(report.by_language.keys().collect::<Vec<_>>(), ["rust"]);

    let case_ids = report
        .cases
        .iter()
        .map(|case| case.query_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(case_ids, ["q-alpha", "q-none"]);
    assert!(report.cases[0].hybrid.is_some());
    assert!(report.cases[0].unanswerable_hybrid.is_none());
    assert!(report.cases[1].hybrid.is_none());
    assert!(report.cases[1].unanswerable_hybrid.is_some());
    assert_eq!(
        report.cases[0].top_paths.first().map(String::as_str),
        Some("src/alpha.rs")
    );
}
