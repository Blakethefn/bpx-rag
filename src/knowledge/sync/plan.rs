use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use rusqlite::params;

use crate::embed::Embed;
use crate::knowledge::{KnowledgeSourceInfo, KnowledgeStore};

use super::classify::{ChangePlan, ScanCounts};
use super::commit::commit_plan;
use super::files::{prepare_file, read_classified_file, sha256_hex, DbFile, PreparedFile};
use super::index::embed_pending_batch;
use super::{EnrichmentFn, SyncResult};

pub(super) fn apply_plan(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    db_files: &HashMap<String, DbFile>,
    plan: ChangePlan,
    enrichment_fn: EnrichmentFn<'_>,
    start: Instant,
    counts: ScanCounts,
) -> Result<SyncResult> {
    let conn = store.conn();
    let embedder = store.embedder();
    let base_path = Path::new(&source.path);
    let files_added = plan.to_add.len();
    let files_updated = plan.to_update.len();
    let files_removed = plan.to_remove.len();
    let unchanged = plan.unchanged;

    let mut prepared_files = Vec::with_capacity(files_added + files_updated);
    for (rel_path, abs_path, hash, stat) in &plan.to_add {
        let content = read_classified_file(abs_path, hash)?;
        prepared_files.push(prepare_file(
            rel_path.clone(),
            abs_path.clone(),
            content,
            hash.clone(),
            None,
            *stat,
            enrichment_fn,
        ));
    }
    for (file_id, abs_path, hash, stat) in &plan.to_update {
        let rel_path = abs_path
            .strip_prefix(base_path)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .to_string();
        let content = read_classified_file(abs_path, hash)?;
        prepared_files.push(prepare_file(
            rel_path,
            abs_path.clone(),
            content,
            hash.clone(),
            Some(*file_id),
            *stat,
            enrichment_fn,
        ));
    }
    let embedding_start = Instant::now();
    let embedding_calls = embed_prepared(&mut prepared_files, embedder.map(|emb| emb.as_ref()))?;
    let embedding_ms = embedding_start.elapsed().as_millis() as u64;
    let chunks_indexed = prepared_files.iter().map(|file| file.chunks.len()).sum();
    let tree_sitter_chunks_indexed = prepared_files
        .iter()
        .filter(|file| file.used_tree_sitter)
        .map(|file| file.chunks.len())
        .sum();
    let existing_chunker_chunks_indexed = chunks_indexed - tree_sitter_chunks_indexed;
    let tree_sitter_files_indexed = prepared_files
        .iter()
        .filter(|file| file.used_tree_sitter)
        .count();
    let existing_chunker_files_indexed = prepared_files.len() - tree_sitter_files_indexed;

    for file in &prepared_files {
        let current_bytes = fs::read(&file.abs_path)
            .with_context(|| format!("failed to recheck {}", file.abs_path.display()))?;
        if sha256_hex(&current_bytes) != file.hash {
            anyhow::bail!(
                "source file changed while embeddings were being prepared: {}; retry sync",
                file.abs_path.display()
            );
        }
    }

    let changed = files_added + files_updated + files_removed > 0;
    let (links_created, transaction_ms) = commit_plan(
        conn,
        source,
        db_files,
        &plan,
        &prepared_files,
        embedder.map(|emb| emb.model_id()),
        changed,
    )?;

    let chunks_total: i64 = conn.query_row(
        "SELECT chunk_count FROM knowledge_sources WHERE id = ?1",
        params![source.id],
        |row| row.get(0),
    )?;

    Ok(SyncResult {
        source_label: source.label.clone(),
        files_scanned: counts.files_scanned,
        sensitive_files_skipped: counts.sensitive_files_skipped,
        skipped_files: counts.skipped,
        files_added,
        files_updated,
        files_removed,
        files_unchanged: unchanged,
        chunks_total: chunks_total as usize,
        chunks_indexed,
        tree_sitter_chunks_indexed,
        existing_chunker_chunks_indexed,
        tree_sitter_files_indexed,
        existing_chunker_files_indexed,
        links_created,
        embedding_ms,
        embedding_calls,
        embedding_chunks: if embedder.is_some() {
            chunks_indexed
        } else {
            0
        },
        transaction_ms,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

fn embed_prepared(
    prepared_files: &mut [PreparedFile],
    embedder: Option<&dyn Embed>,
) -> Result<usize> {
    let Some(emb) = embedder else {
        return Ok(0);
    };
    let batch_size = emb.max_batch_size().clamp(1, 256);
    let mut pending = Vec::with_capacity(batch_size);
    let mut embedding_calls = 0;
    for file in prepared_files.iter_mut() {
        file.vectors = vec![Vec::new(); file.chunks.len()];
    }
    for file_index in 0..prepared_files.len() {
        let chunk_count = prepared_files[file_index].chunks.len();
        for chunk_index in 0..chunk_count {
            let text = {
                let chunk = &prepared_files[file_index].chunks[chunk_index];
                format!("{} {}", chunk.title, chunk.content)
            };
            pending.push((file_index, chunk_index, text));
            if pending.len() == batch_size {
                embed_pending_batch(emb, prepared_files, &mut pending)?;
                embedding_calls += 1;
            }
        }
    }
    if !pending.is_empty() {
        embed_pending_batch(emb, prepared_files, &mut pending)?;
        embedding_calls += 1;
    }
    Ok(embedding_calls)
}
