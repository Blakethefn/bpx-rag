use super::super::super::*;
use crate::search::profile::SearchWhy;

fn passage(file_id: i64, title: &str) -> KnowledgeSearchResult {
    KnowledgeSearchResult {
        title: title.to_string(),
        snippet: String::new(),
        score: 0.0,
        source_label: "repo".to_string(),
        file_path: format!("file-{file_id}.md"),
        file_id,
        source_id: 1,
        lines: "1-1".to_string(),
        metadata: serde_json::json!({}),
        source_type: "knowledge".to_string(),
        why: SearchWhy::default(),
    }
}

fn titles(results: &[KnowledgeSearchResult]) -> Vec<&str> {
    results.iter().map(|result| result.title.as_str()).collect()
}

#[test]
fn cap_one_takes_one_passage_per_file_then_overflow_in_score_order() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(2, "r2"),
        passage(3, "r3"),
        passage(1, "r4"),
        passage(2, "r5"),
    ];

    let output = rank::diversify_by_file(input, 4, 1);

    assert_eq!(titles(&output), ["r0", "r2", "r3", "r1"]);
}

#[test]
fn cap_two_keeps_two_passages_per_file_and_respects_limit() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(2, "r2"),
        passage(1, "r3"),
        passage(2, "r4"),
        passage(3, "r5"),
    ];

    let output = rank::diversify_by_file(input, 4, 2);

    assert_eq!(titles(&output), ["r0", "r1", "r2", "r4"]);
}

#[test]
fn cap_two_fills_from_overflow_when_files_are_scarce() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(1, "r2"),
        passage(2, "r3"),
    ];

    let output = rank::diversify_by_file(input, 4, 2);

    assert_eq!(titles(&output), ["r0", "r1", "r3", "r2"]);
}

#[test]
fn single_file_corpus_returns_up_to_cap_then_fills_to_limit() {
    let input = (0..6)
        .map(|index| passage(1, &format!("r{index}")))
        .collect();

    let output = rank::diversify_by_file(input, 5, 3);

    assert_eq!(titles(&output), ["r0", "r1", "r2", "r3", "r4"]);
}

#[test]
fn cap_above_limit_behaves_as_score_order_truncated_to_limit() {
    let input = vec![
        passage(1, "r0"),
        passage(1, "r1"),
        passage(1, "r2"),
        passage(2, "r3"),
    ];

    let output = rank::diversify_by_file(input, 3, 10);

    assert_eq!(titles(&output), ["r0", "r1", "r2"]);
}

#[test]
fn fewer_passages_than_limit_returns_all_of_them() {
    let input = vec![passage(1, "r0"), passage(2, "r1")];

    let output = rank::diversify_by_file(input, 5, 1);

    assert_eq!(titles(&output), ["r0", "r1"]);
}

#[test]
fn empty_input_and_zero_limit_return_empty() {
    assert!(rank::diversify_by_file(Vec::new(), 5, 1).is_empty());
    assert!(rank::diversify_by_file(vec![passage(1, "r0")], 0, 1).is_empty());
}
