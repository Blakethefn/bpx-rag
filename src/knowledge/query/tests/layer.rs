//! Which layers run for each query shape, and what the diagnostics report.

use super::super::layers::{LayerOutcome, LayerPolicy, LayerReport, SkipReason};
use super::super::*;
use super::seed_chunk;
use crate::embed::Embed;

mod report;
mod gating;

struct BrokenQueryEmbedder;

impl Embed for BrokenQueryEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow::anyhow!("synthetic query embedding failure"))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
    }

    fn dim(&self) -> usize {
        4
    }
}

const ALL_POLICIES: [LayerPolicy; 3] = [
    LayerPolicy::CurrentGating,
    LayerPolicy::OwnPoolGating,
    LayerPolicy::Unconditional,
];

fn lexical_only() -> SearchWeights {
    SearchWeights {
        keyword_weight: 1.0,
        vector_weight: 0.0,
    }
}

fn layer<'a>(report: &'a SearchReport, name: &str) -> &'a LayerReport {
    report
        .layers
        .iter()
        .find(|layer| layer.layer == name)
        .unwrap_or_else(|| panic!("no diagnostics for layer {name}"))
}

fn skipped(reason: SkipReason) -> LayerOutcome {
    LayerOutcome::Skipped { reason }
}

/// Seed `count` chunks in source "repo" that all contain `word`.
fn seed_repeated_word(store: &KnowledgeStore, count: usize, word: &str) {
    for index in 0..count {
        seed_chunk(
            store,
            "repo",
            "/repo",
            &format!("notes/{index}.md"),
            &format!("Note {index}"),
            &format!("entry {index} mentions {word} once"),
            "{}",
        );
    }
}
