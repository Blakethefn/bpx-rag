use super::*;
use crate::embed::{self};
use crate::knowledge;
use crate::knowledge::query::SearchWeights;
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

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
    let sizes = [1_000, 10_000, 50_000];
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
    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(ScalingEmbedder));
    store.add_source("scaling", "/synthetic/bpx-rag-scaling", &[], &[])?;
    let source = store
        .get_source("scaling")?
        .ok_or_else(|| anyhow!("could not load synthetic scaling source"))?;

    let index_start = Instant::now();
    let conn = store.conn();
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let index_result = (|| -> Result<()> {
        for index in 0..chunk_count {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                 VALUES (?1, ?2, 'synthetic', 4096)",
                rusqlite::params![source.id, format!("src/chunk-{index}.rs")],
            )?;
            let file_id = conn.last_insert_rowid();
            let title = format!("synthetic item {index}");
            let content = format!(
                "Benchmark-only unrelated code note with stable filler terms for chunk {index}."
            );
            conn.execute(
                "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
                rusqlite::params![title, content],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
                rusqlite::params![rowid, title, content],
            )?;
            conn.execute(
                "INSERT INTO knowledge_chunk_meta
                 (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, '{}')",
                rusqlite::params![rowid, file_id, source.id, 1, 4],
            )?;
            let vector = ScalingEmbedder::vector_for(index);
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    rowid,
                    embed::embedding_to_bytes(&vector),
                    ScalingEmbedder::DIMENSION as i64,
                    ScalingEmbedder::MODEL_ID
                ],
            )?;
        }
        Ok(())
    })();
    match index_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    let indexing_ms = index_start.elapsed().as_secs_f64() * 1000.0;
    let database_bytes = in_memory_database_bytes(conn)?;
    let weights = SearchWeights {
        keyword_weight: 0.0,
        vector_weight: 1.0,
    };
    let mut exact_latencies = Vec::with_capacity(query_count * repetitions);
    let mut exact_paths_by_query = Vec::with_capacity(query_count);
    for query_index in 0..query_count {
        let query = format!("synthetic semantic query {query_index}");
        let mut exact_paths = Vec::new();
        for _ in 0..repetitions {
            let started = Instant::now();
            let exact_results = knowledge::query::knowledge_search(
                &store,
                &query,
                None,
                Some("scaling"),
                TOP_K as u32,
                &weights,
                128,
            )?;
            exact_latencies.push(started.elapsed());
            exact_paths = unique_paths(
                &exact_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            );
        }
        exact_paths_by_query.push(exact_paths);
    }

    let ann_start = Instant::now();
    store.ensure_vector_index(ScalingEmbedder::MODEL_ID, ScalingEmbedder::DIMENSION)?;
    let ann_build_or_load_ms = ann_start.elapsed().as_secs_f64() * 1000.0;
    let ann_memory_bytes = store.vector_index_memory_usage();
    let index_mode = if ann_memory_bytes == 0 {
        "exact"
    } else {
        "hnsw"
    };
    let mut ann_latencies = Vec::with_capacity(query_count * repetitions);
    let mut recalls = Vec::with_capacity(query_count);
    let mut result_count = 0;
    for (query_index, exact_paths) in exact_paths_by_query.iter().enumerate() {
        let query = format!("synthetic semantic query {query_index}");
        let mut ann_paths = Vec::new();
        for _ in 0..repetitions {
            let started = Instant::now();
            let ann_results = knowledge::query::knowledge_search(
                &store,
                &query,
                None,
                Some("scaling"),
                TOP_K as u32,
                &weights,
                128,
            )?;
            ann_latencies.push(started.elapsed());
            result_count = ann_results.len();
            ann_paths = unique_paths(
                &ann_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            );
        }

        let exact_path_set = exact_paths.iter().collect::<std::collections::HashSet<_>>();
        recalls.push(if exact_paths.is_empty() {
            1.0
        } else {
            ann_paths
                .iter()
                .filter(|path| exact_path_set.contains(path))
                .count() as f64
                / exact_paths.len() as f64
        });
    }

    exact_latencies.sort_by(duration_cmp);
    ann_latencies.sort_by(duration_cmp);
    let exact_latency_ms = exact_latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    let ann_latency_ms = ann_latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    Ok(ScalingPoint {
        chunk_count,
        index_mode,
        database_bytes,
        indexing_ms,
        ann_build_or_load_ms,
        exact_query_p50_ms: percentile(&exact_latency_ms, 0.5),
        exact_query_p95_ms: percentile(&exact_latency_ms, 0.95),
        ann_query_p50_ms: percentile(&ann_latency_ms, 0.5),
        ann_query_p95_ms: percentile(&ann_latency_ms, 0.95),
        ann_recall_at_10: recalls.iter().sum::<f64>() / recalls.len().max(1) as f64,
        ann_memory_bytes,
        result_count,
    })
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

impl embed::Embed for ScalingEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        let query_index = _text
            .split_whitespace()
            .last()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        Ok(Self::vector_for(1_000_000 + query_index))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                let query_index = text
                    .split_whitespace()
                    .last()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                Self::vector_for(1_000_000 + query_index)
            })
            .collect())
    }

    fn dim(&self) -> usize {
        Self::DIMENSION
    }

    fn model_id(&self) -> &str {
        Self::MODEL_ID
    }
}
