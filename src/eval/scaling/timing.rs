use super::super::{duration_cmp, percentile, unique_paths, TOP_K};
use super::index::SyntheticIndex;
use super::{ScalingEmbedder, ScalingPoint};
use crate::knowledge;
use crate::knowledge::query::SearchWeights;
use anyhow::Result;
use std::time::Instant;

pub(super) fn time_queries(
    indexed: SyntheticIndex,
    query_count: usize,
    repetitions: usize,
) -> Result<ScalingPoint> {
    let SyntheticIndex {
        chunk_count,
        mut store,
        indexing_ms,
        database_bytes,
    } = indexed;
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
