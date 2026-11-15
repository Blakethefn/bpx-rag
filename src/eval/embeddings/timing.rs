use super::EmbeddingBenchmarkRun;
use crate::config::EmbeddingDevice;
use crate::embed::{Embed, Embedder};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

pub(super) fn time_runs(
    model_dir: &Path,
    model_name: &str,
    device: EmbeddingDevice,
    thread_counts: &[usize],
    batch_sizes: &[usize],
    texts: &[String],
    warmup_chunks: usize,
    repetitions: usize,
) -> Result<Vec<EmbeddingBenchmarkRun>> {
    let refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
    let mut runs = Vec::with_capacity(thread_counts.len() * batch_sizes.len());
    for &intra_threads in thread_counts {
        for &batch_size in batch_sizes {
            let load_start = Instant::now();
            let model = Embedder::load_cached_with_device(
                model_dir,
                model_name,
                batch_size,
                intra_threads,
                device,
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
    Ok(runs)
}
