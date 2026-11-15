use super::super::format::layer_summary;
use crate::knowledge::query::{LayerOutcome, LayerReport, SkipReason};

#[test]
fn layer_summary_names_each_state_and_skip_reason() {
    let layers = vec![
        LayerReport {
            layer: "bm25",
            outcome: LayerOutcome::Ran,
            candidates: 12,
        },
        LayerReport {
            layer: "trigram",
            outcome: LayerOutcome::Skipped {
                reason: SkipReason::PoolFull,
            },
            candidates: 0,
        },
        LayerReport {
            layer: "vector",
            outcome: LayerOutcome::Failed {
                error: "model unavailable".to_string(),
            },
            candidates: 0,
        },
    ];

    assert_eq!(
        layer_summary(&layers),
        "layers: bm25 ran (12 candidates); trigram skipped (candidate pool full); vector failed (model unavailable)\n"
    );
}
