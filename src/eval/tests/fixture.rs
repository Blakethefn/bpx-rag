use super::super::*;
use crate::config::Config;
use crate::embed;
use crate::knowledge::query::{LayerPolicy, SearchWeights};

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
