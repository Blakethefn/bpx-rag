use crate::embed::{self};
use anyhow::{Result, anyhow};
use serde::Serialize;

mod index;
mod timing;

#[derive(Debug, Serialize)]
pub struct ScalingPoint {
    pub chunk_count: usize,
    pub index_mode: &'static str,
    pub database_bytes: u64,
    pub indexing_ms: f64,
    pub ann_build_or_load_ms: f64,
    pub exact_query_p50_ms: f64,
    pub exact_query_p95_ms: f64,
    pub ann_query_p50_ms: f64,
    pub ann_query_p95_ms: f64,
    pub ann_recall_at_10: f64,
    pub ann_memory_bytes: usize,
    pub result_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ScalingEvalReport {
    pub embedding_dimension: usize,
    pub query_count: usize,
    pub repetitions: usize,
    pub points: Vec<ScalingPoint>,
}

pub fn run_scaling_evaluation(repetitions: usize) -> Result<ScalingEvalReport> {
    if !(1..=100).contains(&repetitions) {
        return Err(anyhow!("repetitions must be between 1 and 100"));
    }
    let sizes = [1_000, 4_096, 10_000, 16_384, 25_000, 50_000];
    const QUERY_COUNT: usize = 5;
    let mut points = Vec::with_capacity(sizes.len());
    for chunk_count in sizes {
        points.push(run_scaling_point(chunk_count, QUERY_COUNT, repetitions)?);
    }
    Ok(ScalingEvalReport {
        embedding_dimension: ScalingEmbedder::DIMENSION,
        query_count: QUERY_COUNT,
        repetitions,
        points,
    })
}

pub(super) fn run_scaling_point(
    chunk_count: usize,
    query_count: usize,
    repetitions: usize,
) -> Result<ScalingPoint> {
    let indexed = index::index_synthetic(chunk_count)?;
    timing::time_queries(indexed, query_count, repetitions)
}

pub(super) struct ScalingEmbedder;

impl ScalingEmbedder {
    const DIMENSION: usize = 384;
    const MODEL_ID: &'static str = "bpx-rag-scaling-benchmark-v1";

    fn vector_for(index: usize) -> Vec<f32> {
        let mut state = (index as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut vector = (0..Self::DIMENSION)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as f32 / ((1_u32 << 24) as f32)) - 0.5
            })
            .collect::<Vec<_>>();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        vector.iter_mut().for_each(|value| *value /= norm);
        vector
    }
}

fn parse_query_index(text: &str) -> usize {
    text.split_whitespace()
        .last()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default()
}

impl embed::Embed for ScalingEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::vector_for(1_000_000 + parse_query_index(text)))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| Self::vector_for(1_000_000 + parse_query_index(text)))
            .collect())
    }

    fn dim(&self) -> usize {
        Self::DIMENSION
    }

    fn model_id(&self) -> &str {
        Self::MODEL_ID
    }
}
