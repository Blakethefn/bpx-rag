use super::super::sha256;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub(super) struct CorpusSample {
    pub(super) files_scanned: usize,
    pub(super) sensitive_files_skipped: usize,
    pub(super) tree_sitter_files: usize,
    pub(super) existing_chunker_files: usize,
    pub(super) tree_sitter_chunks: usize,
    pub(super) existing_chunker_chunks: usize,
    pub(super) available_chunks: usize,
    pub(super) texts: Vec<String>,
}

pub(super) fn sample_chunks(
    corpus_root: &Path,
    glob_pattern: Option<&str>,
    max_chunks: usize,
) -> Result<CorpusSample> {
    let patterns: Vec<String> = glob_pattern
        .map(|pattern| vec![pattern.to_string()])
        .unwrap_or_default();
    let walk = crate::knowledge::sync::walk_source_dir_with_report(corpus_root, &patterns)?;
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
        let content = fs::read_to_string(path)
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
    Ok(CorpusSample {
        files_scanned,
        sensitive_files_skipped,
        tree_sitter_files,
        existing_chunker_files,
        tree_sitter_chunks,
        existing_chunker_chunks,
        available_chunks,
        texts,
    })
}
