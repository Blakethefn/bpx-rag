use super::*;
use std::collections::HashMap;

fn hit(rowid: i64) -> RankedResult {
    RankedResult {
        rowid,
        file_id: 1,
        source_id: 1,
        title: "title".to_string(),
        content: "content".to_string(),
        rank: 0.0,
        source_label: "repo".to_string(),
        file_path: "a.md".to_string(),
        line_start: 1,
        line_end: 1,
        metadata: "{}".to_string(),
    }
}

/// Rank 0 of a weight-1 layer. K is 60, so the term is 1/61.
fn first_term() -> f64 {
    1.0 / 61.0
}

fn why_line(name: &str, score: f64) -> String {
    format!("why: {name}={score:.4}, final={score:.4}")
}

#[test]
fn catalog_names_are_the_ranker_strings_in_pipeline_order() {
    let names: Vec<&str> = LAYERS.iter().map(|layer| layer.name).collect();
    assert_eq!(names, ["bm25", "trigram", "fuzzy", "identifier", "vector"]);
}

#[test]
fn keyword_layers_are_the_zero_weight_skip_list() {
    let names: Vec<&str> = keyword_layers().map(|layer| layer.name).collect();
    assert_eq!(names, ["bm25", "trigram", "fuzzy", "identifier"]);
}

#[test]
fn each_layer_records_its_score_on_the_first_hit_and_adds_it_on_a_later_hit() {
    for layer in LAYERS {
        let mut acc = HashMap::new();
        merge_results(&mut acc, &[hit(7)], 1.0, *layer);

        let scored = acc.get(&7).expect(layer.name);
        let first = first_term();
        assert_eq!(scored.rrf_score, first, "{}", layer.name);
        assert_eq!(
            scored.why.detailed_line(scored.rrf_score),
            why_line(layer.name, first),
            "{}",
            layer.name
        );

        merge_results(&mut acc, &[hit(7)], 1.0, *layer);
        let scored = acc.get(&7).expect(layer.name);
        let total = first + first;
        assert_eq!(scored.rrf_score, total, "{}", layer.name);
        assert_eq!(
            scored.why.detailed_line(scored.rrf_score),
            why_line(layer.name, total),
            "{}",
            layer.name
        );
    }
}

#[test]
fn a_later_layer_keeps_the_score_recorded_on_the_first_hit() {
    let mut acc = HashMap::new();
    merge_results(&mut acc, &[hit(7)], 1.0, BM25);
    merge_results(&mut acc, &[hit(7)], 1.0, VECTOR);

    let scored = acc.get(&7).expect("merged row");
    let term = first_term();
    assert_eq!(scored.rrf_score, term + term);
    assert_eq!(scored.why.layers.bm25, term);
    assert_eq!(scored.why.layers.vector, term);
    assert_eq!(scored.why.layers.trigram, 0.0);
    assert_eq!(scored.why.layers.fuzzy, 0.0);
    assert_eq!(scored.why.layers.identifier, 0.0);
    assert_eq!(scored.why.layers.matched_layers(), vec!["bm25", "vector"]);
}
