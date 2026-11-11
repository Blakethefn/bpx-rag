use super::*;

fn sample_why() -> SearchWhy {
    SearchWhy {
        layers: LayerScores {
            bm25: 0.1,
            trigram: 0.0,
            fuzzy: 0.2,
            identifier: 0.0,
            vector: 0.3,
            proximity: 0.0,
            knowledge_bias: 0.4,
            graph_boost: 0.5,
        },
        metadata_match: true,
        graph_neighbor: true,
    }
}

#[test]
fn matched_layers_keep_display_labels() {
    assert_eq!(
        sample_why().layers.matched_layers(),
        vec!["bm25", "fuzzy", "vector", "knowledge-bias", "graph"]
    );
}

#[test]
fn compact_line_lists_positive_layers_and_flags() {
    assert_eq!(
        sample_why().compact_line(),
        "why: bm25, fuzzy, vector, knowledge-bias, graph, metadata, graph-neighbor"
    );
}

#[test]
fn detailed_line_formats_scores_and_final() {
    assert_eq!(
        sample_why().detailed_line(1.5),
        "why: bm25=0.1000, fuzzy=0.2000, vector=0.3000, knowledge-bias=0.4000, graph=0.5000, metadata=true, graph-neighbor=true, final=1.5000"
    );
}

#[test]
fn empty_why_uses_ranking_defaults() {
    let why = SearchWhy::default();
    assert_eq!(why.compact_line(), "why: ranking-defaults");
    assert_eq!(why.detailed_line(0.0), "why: final=0.0000");
}
