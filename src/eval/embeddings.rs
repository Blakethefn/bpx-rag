use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::{self};
use std::path::Path;
use std::time::Instant;
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use super::*;

#[derive(Debug, Serialize)]
pub struct EmbeddingBenchmarkReport {
    pub model_name: String,
    pub model_id: String,
    pub embedding_device: String,
    pub source_glob: Option<String>,
    pub corpus_root_name: String,
    pub host_threads: usize,
    pub files_scanned: usize,
    pub sensitive_files_skipped: usize,
    pub tree_sitter_files: usize,
    pub existing_chunker_files: usize,
    pub tree_sitter_chunks: usize,
    pub existing_chunker_chunks: usize,
    pub available_chunks: usize,
    pub measured_chunks: usize,
    pub warmup_chunks: usize,
    pub repetitions: usize,
    pub runs: Vec<EmbeddingBenchmarkRun>,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingBenchmarkRun {
    pub intra_threads: usize,
    pub batch_size: usize,
    pub model_load_ms: f64,
    pub warmup_ms: f64,
    pub embedding_ms_samples: Vec<f64>,
    pub embedding_ms: f64,
    pub chunks_per_second: f64,
    pub milliseconds_per_chunk: f64,
}

/// Compare cached ONNX model throughput over a deterministic sample produced by
/// the same ignore rules and chunker used by persistent knowledge indexing.
pub fn run_embedding_benchmark(
    corpus_root: &Path,
    glob_pattern: Option<&str>,
    thread_counts: &[usize],
    batch_sizes: &[usize],
    repetitions: usize,
    max_chunks: usize,
) -> Result<EmbeddingBenchmarkReport> {
    let host_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 256);
    let thread_counts = if thread_counts.is_empty() {
        default_thread_sweep(host_threads)
    } else {
        thread_counts.to_vec()
    };
    if !corpus_root.is_dir() {
        anyhow::bail!(
            "benchmark corpus is not a directory: {}",
            corpus_root.display()
        );
    }
    if thread_counts.iter().any(|value| !(1..=256).contains(value))
        || batch_sizes.is_empty()
        || batch_sizes.iter().any(|value| !(1..=256).contains(value))
    {
        anyhow::bail!("thread and batch-size values must be between 1 and 256");
    }
    if max_chunks == 0 || max_chunks > 100_000 {
        anyhow::bail!("max-chunks must be between 1 and 100000");
    }
    if !(1..=10).contains(&repetitions) {
        anyhow::bail!("repetitions must be between 1 and 10");
    }
    if thread_counts.len().saturating_mul(batch_sizes.len()) > 64 {
        anyhow::bail!("embedding benchmark is limited to 64 thread/batch combinations");
    }

    let config = Config::load()?;
    let model_dir = crate::app::model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        anyhow::bail!(
            "embedding benchmark requires the configured model to be cached; run `bpx-rag models download` first ({})",
            model_dir.display()
        );
    }

    let walk = crate::knowledge::sync::walk_source_dir_with_report(corpus_root, glob_pattern)?;
    let sensitive_files_skipped = walk.sensitive_files_skipped;
    let mut files = walk.files.into_iter().collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let files_scanned = files.len();
    let mut available_chunks = 0usize;
    let mut tree_sitter_chunks = 0usize;
    let mut existing_chunker_chunks = 0usize;
    let mut tree_sitter_files = 0usize;
    let mut existing_chunker_files = 0usize;
    let mut file_hashes = Vec::with_capacity(files.len());
    for (_, path) in &files {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read benchmark file {}", path.display()))?;
        let result = crate::chunk::chunk_content_for_path(path, &content);
        let chunk_count = result.chunks.len();
        available_chunks += chunk_count;
        if result.used_tree_sitter {
            tree_sitter_files += 1;
            tree_sitter_chunks += chunk_count;
        } else {
            existing_chunker_files += 1;
            existing_chunker_chunks += chunk_count;
        }
        file_hashes.push(sha256(&content));
    }
    if available_chunks == 0 {
        anyhow::bail!("benchmark corpus contains no indexable chunks");
    }
    let measured_chunks = available_chunks.min(max_chunks);
    let sample_indices = if measured_chunks == 1 {
        vec![available_chunks / 2]
    } else {
        (0..measured_chunks)
            .map(|position| {
                (position as u128 * (available_chunks - 1) as u128 / (measured_chunks - 1) as u128)
                    as usize
            })
            .collect::<Vec<_>>()
    };
    let mut texts = Vec::with_capacity(measured_chunks);
    let mut next_sample = 0;
    let mut chunk_index = 0;
    for ((_, path), expected_hash) in files.iter().zip(&file_hashes) {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read benchmark file {}", path.display()))?;
        if sha256(&content) != *expected_hash {
            anyhow::bail!(
                "benchmark source changed while chunks were being sampled: {}; retry evaluation",
                path.display()
            );
        }
        for chunk in crate::chunk::chunk_content_for_path(path, &content).chunks {
            if sample_indices.get(next_sample) == Some(&chunk_index) {
                texts.push(format!("{} {}", chunk.title, chunk.content));
                next_sample += 1;
            }
            chunk_index += 1;
        }
    }
    if texts.len() != measured_chunks || chunk_index != available_chunks {
        anyhow::bail!("benchmark corpus changed while chunks were being sampled; retry evaluation");
    }

    let model_id = embed::canonical_model_id(&config.embeddings.model)?;
    let warmup_chunks = texts.len().min(8);
    let refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
    let mut runs = Vec::with_capacity(thread_counts.len() * batch_sizes.len());

    for intra_threads in thread_counts {
        for &batch_size in batch_sizes {
            let load_start = Instant::now();
            let model = Embedder::load_cached_with_device(
                &model_dir,
                &config.embeddings.model,
                batch_size,
                intra_threads,
                config.embeddings.device,
            )?;
            let model_load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

            let warmup_start = Instant::now();
            let warmup = model.embed_batch(&refs[..warmup_chunks])?;
            let warmup_ms = warmup_start.elapsed().as_secs_f64() * 1000.0;
            if warmup.len() != warmup_chunks {
                anyhow::bail!("embedding provider returned an incomplete warmup result");
            }

            let mut embedding_ms_samples = Vec::with_capacity(repetitions);
            for _ in 0..repetitions {
                let embedding_start = Instant::now();
                let vectors = model.embed_batch(&refs)?;
                let embedding_ms = embedding_start.elapsed().as_secs_f64() * 1000.0;
                if vectors.len() != refs.len() || vectors.iter().any(|v| v.len() != model.dim()) {
                    anyhow::bail!("embedding provider returned invalid vector shapes");
                }
                embedding_ms_samples.push(embedding_ms);
            }
            let mut sorted_samples = embedding_ms_samples.clone();
            sorted_samples.sort_by(f64::total_cmp);
            let embedding_ms = if sorted_samples.len() % 2 == 0 {
                let middle = sorted_samples.len() / 2;
                (sorted_samples[middle - 1] + sorted_samples[middle]) / 2.0
            } else {
                sorted_samples[sorted_samples.len() / 2]
            };
            let elapsed_seconds = (embedding_ms / 1000.0).max(f64::EPSILON);
            runs.push(EmbeddingBenchmarkRun {
                intra_threads,
                batch_size,
                model_load_ms,
                warmup_ms,
                embedding_ms_samples,
                embedding_ms,
                chunks_per_second: refs.len() as f64 / elapsed_seconds,
                milliseconds_per_chunk: embedding_ms / refs.len() as f64,
            });
        }
    }

    Ok(EmbeddingBenchmarkReport {
        model_name: config.embeddings.model,
        model_id,
        embedding_device: config.embeddings.device.as_str().to_string(),
        source_glob: glob_pattern.map(str::to_string),
        corpus_root_name: corpus_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("corpus")
            .to_string(),
        host_threads,
        files_scanned,
        sensitive_files_skipped,
        tree_sitter_files,
        existing_chunker_files,
        tree_sitter_chunks,
        existing_chunker_chunks,
        available_chunks,
        measured_chunks: texts.len(),
        warmup_chunks,
        repetitions,
        runs,
    })
}

pub(super) fn default_thread_sweep(host_threads: usize) -> Vec<usize> {
    let candidates = [
        host_threads.div_ceil(3),
        host_threads.saturating_mul(2).div_ceil(3),
        host_threads,
    ];
    let mut values = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if values.last().copied() != Some(candidate) {
            values.push(candidate);
        }
    }
    values
}
