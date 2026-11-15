use crate::config::Config;
use crate::embed::{self};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

mod corpus;
mod timing;

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

    let sample = corpus::sample_chunks(corpus_root, glob_pattern, max_chunks)?;
    let model_id = embed::canonical_model_id(&config.embeddings.model)?;
    let warmup_chunks = sample.texts.len().min(8);
    let runs = timing::time_runs(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.device,
        &thread_counts,
        batch_sizes,
        &sample.texts,
        warmup_chunks,
        repetitions,
    )?;

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
        files_scanned: sample.files_scanned,
        sensitive_files_skipped: sample.sensitive_files_skipped,
        tree_sitter_files: sample.tree_sitter_files,
        existing_chunker_files: sample.existing_chunker_files,
        tree_sitter_chunks: sample.tree_sitter_chunks,
        existing_chunker_chunks: sample.existing_chunker_chunks,
        available_chunks: sample.available_chunks,
        measured_chunks: sample.texts.len(),
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
