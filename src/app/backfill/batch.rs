use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::embed::{self, Embed};

type PendingRow = (i64, String, String, i64);

pub(super) fn pending_batch(
    conn: &Connection,
    model_id: &str,
    source_id: Option<i64>,
    batch_size: usize,
) -> Result<Vec<PendingRow>> {
    let row_mapper = |row: &rusqlite::Row<'_>| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    };
    match source_id {
        Some(source_id) => {
            let mut statement = conn.prepare(
                "SELECT kc.rowid, kc.title, kc.content, m.source_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE (e.chunk_rowid IS NULL OR e.model_id <> ?1)
                   AND m.source_id = ?2
                 ORDER BY kc.rowid
                 LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![model_id, source_id, batch_size as i64], row_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }
        None => {
            let mut statement = conn.prepare(
                "SELECT kc.rowid, kc.title, kc.content, m.source_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE e.chunk_rowid IS NULL OR e.model_id <> ?1
                 ORDER BY kc.rowid
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![model_id, batch_size as i64], row_mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }
    }
}

pub(super) fn write_batch(
    conn: &Connection,
    embedder: &dyn Embed,
    batch: &[PendingRow],
    vectors: &[Vec<f32>],
) -> Result<(usize, usize)> {
    let mut written = 0usize;
    let mut skipped = 0usize;
    for ((rowid, expected_title, expected_content, expected_source_id), vector) in
        batch.iter().zip(vectors.iter())
    {
        let current: Option<(String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT kc.title, kc.content, m.source_id, e.model_id
                 FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta m ON m.chunk_rowid = kc.rowid
                 LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kc.rowid
                 WHERE kc.rowid = ?1",
                params![rowid],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((title, content, current_source_id, current_model_id)) = current else {
            skipped += 1;
            continue;
        };
        if title != *expected_title
            || content != *expected_content
            || current_source_id != *expected_source_id
            || current_model_id.as_deref() == Some(embedder.model_id())
        {
            skipped += 1;
            continue;
        }

        let vector = embed::normalize_embedding(vector, embedder.dim())?;
        conn.execute(
            "INSERT OR REPLACE INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                *rowid,
                embed::embedding_to_bytes(&vector),
                embedder.dim() as i64,
                embedder.model_id()
            ],
        )?;
        written += 1;
    }
    Ok((written, skipped))
}
