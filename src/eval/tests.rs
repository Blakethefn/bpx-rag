use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

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
