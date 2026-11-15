use anyhow::Result;

use crate::embed::Embed;

use super::super::files::PreparedFile;
use super::super::index::embed_pending_batch;

pub(super) fn embed_prepared(
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
