//! Incremental sync engine for the knowledge store.
//!
//! Walks registered knowledge source directories, compares file content hashes
//! against the database, and re-indexes only changed files. Provides
//! `reindex_file()` for single-file updates used by the write-through hook (P05).

use anyhow::{Context, Result};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, io};

use crate::embedder::{embedding_to_bytes, normalize_embedding};
use crate::store::chunker::chunk_content;

use super::{KnowledgeSourceInfo, KnowledgeStore};

/// Result of a knowledge sync operation.
#[derive(Debug, serde::Serialize)]
pub struct SyncResult {
    pub source_label: String,
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_unchanged: usize,
    pub chunks_total: usize,
    /// Chunks prepared and written by this sync operation.
    pub chunks_indexed: usize,
    pub links_created: usize,
    /// Time spent waiting for and running the embedding model, in milliseconds.
    pub embedding_ms: u64,
    /// Number of corpus-wide calls made through the Embed trait.
    pub embedding_calls: usize,
    /// Number of chunk texts embedded before writing this sync transaction.
    pub embedding_chunks: usize,
    /// Time spent in the SQLite write transaction, in milliseconds.
    pub transaction_ms: u64,
    pub duration_ms: u64,
}

struct PreparedFile {
    rel_path: String,
    abs_path: PathBuf,
    content: String,
    hash: String,
    file_id: Option<i64>,
    metadata: String,
    chunks: Vec<crate::store::chunker::Chunk>,
    vectors: Vec<Vec<f32>>,
}

/// Sync a single knowledge source. Walks the directory, hashes files,
/// and re-indexes only changed/new files. Removes chunks for deleted files.
/// Prepared-file edits are revalidated after embedding; edits to unselected
/// files during the walk are picked up by the next sync.
pub fn sync_source(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: Option<&dyn Fn(&str, &Path) -> serde_json::Value>,
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let embedder = store.embedder();
    let base_path = Path::new(&source.path);

    // 1. Walk directory (respects .gitignore, skips binary/empty files)
    let disk_files = walk_source_dir(base_path, source.glob.as_deref())?;

    // 2. Load existing file records from DB
    let db_files = load_db_files(conn, source.id)?;

    // 3. Classify files: read once, hash, decode UTF-8, then classify
    let mut to_add: Vec<(String, PathBuf, String, String)> = Vec::new(); // (rel, abs, content, hash)
    let mut to_update: Vec<(i64, PathBuf, String, String)> = Vec::new(); // (file_id, abs, content, hash)
    let mut to_remove: Vec<i64> = Vec::new();
    let mut unchanged: usize = 0;

    for (rel_path, abs_path) in &disk_files {
        let bytes =
            fs::read(abs_path).with_context(|| format!("failed to read {}", abs_path.display()))?;

        // Abort before the transaction if a candidate file cannot be indexed.
        // Silently skipping it can leave stale indexed content behind.
        let content = String::from_utf8(bytes)
            .with_context(|| format!("file is not valid UTF-8: {}", abs_path.display()))?;
        let hash = sha256_hex(content.as_bytes());

        if let Some((file_id, old_hash)) = db_files.get(rel_path) {
            if *old_hash != hash {
                to_update.push((*file_id, abs_path.clone(), content, hash));
            } else {
                unchanged += 1;
            }
        } else {
            to_add.push((rel_path.clone(), abs_path.clone(), content, hash));
        }
    }

    for (rel_path, (file_id, _)) in &db_files {
        if !disk_files.contains_key(rel_path) {
            to_remove.push(*file_id);
        }
    }

    let files_scanned = disk_files.len();
    let files_added = to_add.len();
    let files_updated = to_update.len();
    let files_removed = to_remove.len();

    // Prepare all changed files and embeddings before acquiring SQLite's write
    // lock. The embedder applies its configured internal batch size to this
    // corpus-wide call, avoiding one model invocation per file.
    let mut prepared_files = Vec::with_capacity(files_added + files_updated);
    let mut embedding_texts = Vec::new();
    for (rel_path, abs_path, content, hash) in &to_add {
        prepared_files.push(prepare_file(
            rel_path.clone(),
            abs_path.clone(),
            content.clone(),
            hash.clone(),
            None,
            enrichment_fn,
        ));
    }
    for (file_id, abs_path, content, hash) in &to_update {
        let rel_path = abs_path
            .strip_prefix(base_path)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .to_string();
        prepared_files.push(prepare_file(
            rel_path,
            abs_path.clone(),
            content.clone(),
            hash.clone(),
            Some(*file_id),
            enrichment_fn,
        ));
    }
    for file in &prepared_files {
        embedding_texts.extend(
            file.chunks
                .iter()
                .map(|chunk| format!("{} {}", chunk.title, chunk.content)),
        );
    }

    let embedding_start = Instant::now();
    let embedding_calls = usize::from(embedder.is_some() && !embedding_texts.is_empty());
    if let Some(emb) = embedder.filter(|_| !embedding_texts.is_empty()) {
        let refs: Vec<&str> = embedding_texts.iter().map(String::as_str).collect();
        let embeddings = emb
            .embed_batch(&refs)
            .context("failed to embed changed corpus chunks")?;
        if embeddings.len() != embedding_texts.len() {
            anyhow::bail!(
                "embedder returned {} vectors for {} chunks",
                embeddings.len(),
                embedding_texts.len()
            );
        }
        let normalized = embeddings
            .iter()
            .map(|vector| normalize_embedding(vector, emb.dim()))
            .collect::<Result<Vec<_>>>()?;
        let mut vectors = normalized.into_iter();
        for file in &mut prepared_files {
            file.vectors = vectors.by_ref().take(file.chunks.len()).collect();
        }
    }
    let embedding_ms = embedding_start.elapsed().as_millis() as u64;
    let chunks_indexed = prepared_files.iter().map(|file| file.chunks.len()).sum();

    // Avoid committing vectors for file contents that changed during a long
    // embedding pass. The next sync can retry against the newer snapshot.
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

    // Apply all database changes in one transaction after CPU/model work.
    let transaction_start = Instant::now();
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let inner = (|| -> Result<usize> {
        // Another process may have synced/reindexed this source while model
        // inference ran. Refuse to apply work based on a stale file-row map.
        if load_db_files(conn, source.id)? != db_files {
            anyhow::bail!("knowledge source changed during sync preparation; retry sync");
        }

        // 4a. Remove deleted files
        for file_id in &to_remove {
            delete_file_chunks(conn, *file_id)?;
            conn.execute(
                "DELETE FROM knowledge_files WHERE id = ?1",
                params![file_id],
            )?;
        }

        // 4b. Replace changed files and add new files using prepared vectors.
        for file in &prepared_files {
            let file_id = match file.file_id {
                Some(file_id) => {
                    delete_file_chunks(conn, file_id)?;
                    file_id
                }
                None => {
                    conn.execute(
                        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![source.id, file.rel_path, file.hash, file.content.len() as i64],
                    )?;
                    conn.last_insert_rowid()
                }
            };
            index_prepared_file(
                conn,
                source.id,
                file_id,
                file,
                embedder.map(|emb| emb.model_id()),
            )?;
        }

        // 4d. Update source stats
        conn.execute(
            "UPDATE knowledge_sources SET
                last_sync = datetime('now'),
                file_count = (SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1),
                chunk_count = (SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1)
             WHERE id = ?1",
            params![source.id],
        )?;

        // 4e. Resolve wikilinks to graph edges
        let lc = resolve_wikilinks_for_source(conn, source.id)?;

        Ok(lc)
    })();

    let links_created = match inner {
        Ok(links_created) => {
            conn.execute_batch("COMMIT")?;
            links_created
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };
    let transaction_ms = transaction_start.elapsed().as_millis() as u64;

    let chunks_total: i64 = conn.query_row(
        "SELECT chunk_count FROM knowledge_sources WHERE id = ?1",
        params![source.id],
        |row| row.get(0),
    )?;

    Ok(SyncResult {
        source_label: source.label.clone(),
        files_scanned,
        files_added,
        files_updated,
        files_removed,
        files_unchanged: unchanged,
        chunks_total: chunks_total as usize,
        chunks_indexed,
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

/// Sync all registered knowledge sources.
pub fn sync_all(
    store: &KnowledgeStore,
    enrichment_fn: Option<&dyn Fn(&str, &Path) -> serde_json::Value>,
) -> Result<Vec<SyncResult>> {
    let sources = store.list_sources()?;
    let mut results = Vec::with_capacity(sources.len());
    for source in &sources {
        results.push(sync_source(store, source, enrichment_fn)?);
    }
    Ok(results)
}

/// Re-index a single file within a knowledge source.
/// Used by the write-through hook for immediate updates.
pub fn reindex_file(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    abs_path: &Path,
    enrichment_fn: Option<&dyn Fn(&str, &Path) -> serde_json::Value>,
) -> Result<()> {
    let conn = store.conn();
    let embedder = store.embedder();
    let base_path = Path::new(&source.path);

    let rel_path = abs_path
        .strip_prefix(base_path)
        .context("file path is not inside the source directory")?
        .to_string_lossy()
        .to_string();

    let bytes =
        fs::read(abs_path).with_context(|| format!("failed to read {}", abs_path.display()))?;
    let content =
        String::from_utf8(bytes).context("file content is not valid UTF-8 for reindex")?;
    let hash = sha256_hex(content.as_bytes());

    // Check if file exists and hash matches — skip early if unchanged
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, content_hash FROM knowledge_files
             WHERE source_id = ?1 AND rel_path = ?2",
            params![source.id, rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    if let Some((_, ref old_hash)) = existing {
        if *old_hash == hash {
            return Ok(()); // No change
        }
    }

    let mut prepared = prepare_file(
        rel_path.clone(),
        abs_path.to_path_buf(),
        content.clone(),
        hash.clone(),
        existing.as_ref().map(|(file_id, _)| *file_id),
        enrichment_fn,
    );
    if let Some(emb) = embedder {
        let texts: Vec<String> = prepared
            .chunks
            .iter()
            .map(|chunk| format!("{} {}", chunk.title, chunk.content))
            .collect();
        if !texts.is_empty() {
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let vectors = emb.embed_batch(&refs)?;
            if vectors.len() != texts.len() {
                anyhow::bail!(
                    "embedder returned {} vectors for {} chunks",
                    vectors.len(),
                    texts.len()
                );
            }
            prepared.vectors = vectors
                .iter()
                .map(|vector| normalize_embedding(vector, emb.dim()))
                .collect::<Result<Vec<_>>>()?;
        }
    }

    let current_content =
        fs::read(abs_path).with_context(|| format!("failed to recheck {}", abs_path.display()))?;
    if sha256_hex(&current_content) != hash {
        anyhow::bail!(
            "source file changed while embeddings were being prepared: {}; retry reindex",
            abs_path.display()
        );
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;

    let inner = (|| -> Result<()> {
        let current: Option<(i64, String)> = conn
            .query_row(
                "SELECT id, content_hash FROM knowledge_files
                 WHERE source_id = ?1 AND rel_path = ?2",
                params![source.id, rel_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if current.as_ref() != existing.as_ref() {
            anyhow::bail!("file index changed while reindex was preparing; retry reindex");
        }

        if let Some((file_id, _)) = existing {
            delete_file_chunks(conn, file_id)?;
            index_prepared_file(
                conn,
                source.id,
                file_id,
                &prepared,
                embedder.map(|emb| emb.model_id()),
            )?;
        } else {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source.id, rel_path, hash, content.len() as i64],
            )?;
            let file_id = conn.last_insert_rowid();
            index_prepared_file(
                conn,
                source.id,
                file_id,
                &prepared,
                embedder.map(|emb| emb.model_id()),
            )?;
        }

        conn.execute(
            "UPDATE knowledge_sources SET
                file_count = (SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1),
                chunk_count = (SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1)
             WHERE id = ?1",
            params![source.id],
        )?;

        Ok(())
    })();

    match inner {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }

    Ok(())
}

// ── Internal helpers ────────────────────────────────────────────────────────

/// Walk a knowledge source directory, returning files keyed by relative path.
/// Respects .gitignore, skips binary/empty files, and applies the glob filter.
/// Any traversal or inspection error aborts the sync so reconciliation cannot
/// mistake an incomplete scan for deleted files.
pub(crate) fn walk_source_dir(
    base_path: &Path,
    glob_pattern: Option<&str>,
) -> Result<HashMap<String, PathBuf>> {
    if !base_path.is_dir() {
        return Err(anyhow::anyhow!(
            "knowledge source path is not a directory: {}",
            base_path.display()
        ));
    }

    let mut builder = WalkBuilder::new(base_path);
    builder
        .hidden(false) // Don't skip hidden files (dotfiles may be relevant)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    if let Some(pattern) = glob_pattern {
        let mut overrides = OverrideBuilder::new(base_path);
        overrides.add(pattern).context("invalid glob pattern")?;
        builder.overrides(overrides.build()?);
    }

    let mut files = HashMap::new();

    for entry in builder.build() {
        let entry = entry.context("failed while traversing knowledge source")?;

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let abs_path = entry.path().to_path_buf();

        // Skip binary files (null byte in first 1KB)
        if is_binary(&abs_path)
            .with_context(|| format!("failed to inspect {}", abs_path.display()))?
        {
            continue;
        }

        // Skip empty files
        let metadata = fs::metadata(&abs_path)
            .with_context(|| format!("failed to inspect {}", abs_path.display()))?;
        if metadata.len() == 0 {
            continue;
        }

        let rel_path = abs_path
            .strip_prefix(base_path)
            .unwrap_or(&abs_path)
            .to_string_lossy()
            .to_string();

        files.insert(rel_path, abs_path);
    }

    Ok(files)
}

/// Compute SHA-256 hex digest of bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Load existing file records from the knowledge DB for a source.
fn load_db_files(
    conn: &rusqlite::Connection,
    source_id: i64,
) -> Result<HashMap<String, (i64, String)>> {
    let mut stmt = conn
        .prepare("SELECT id, rel_path, content_hash FROM knowledge_files WHERE source_id = ?1")?;
    let mut files = HashMap::new();
    let rows = stmt.query_map(params![source_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, rel_path, hash) = row?;
        files.insert(rel_path, (id, hash));
    }
    Ok(files)
}

fn prepare_file(
    rel_path: String,
    abs_path: PathBuf,
    content: String,
    hash: String,
    file_id: Option<i64>,
    enrichment_fn: Option<&dyn Fn(&str, &Path) -> serde_json::Value>,
) -> PreparedFile {
    let metadata = match enrichment_fn {
        Some(f) => f(&content, &abs_path),
        None => serde_json::json!({}),
    };
    let chunks = chunk_content(&content);
    PreparedFile {
        rel_path,
        abs_path,
        content,
        hash,
        file_id,
        metadata: metadata.to_string(),
        chunks,
        vectors: Vec::new(),
    }
}

/// Insert pre-chunked content and precomputed vectors within the caller's
/// transaction. All expensive enrichment and model inference happened earlier.
fn index_prepared_file(
    conn: &rusqlite::Connection,
    source_id: i64,
    file_id: i64,
    file: &PreparedFile,
    model_id: Option<&str>,
) -> Result<usize> {
    if model_id.is_some() && file.vectors.len() != file.chunks.len() {
        anyhow::bail!("prepared embedding count does not match prepared chunk count");
    }
    for (chunk_index, chunk) in file.chunks.iter().enumerate() {
        // FTS5 (BM25)
        conn.execute(
            "INSERT INTO knowledge_chunks(title, content) VALUES (?1, ?2)",
            params![chunk.title, chunk.content],
        )?;
        let rowid = conn.last_insert_rowid();

        // Trigram FTS5 with explicit rowid
        conn.execute(
            "INSERT INTO knowledge_chunks_trigram(rowid, title, content) VALUES (?1, ?2, ?3)",
            params![rowid, chunk.title, chunk.content],
        )?;

        // Chunk metadata
        conn.execute(
            "INSERT INTO knowledge_chunk_meta
                (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rowid,
                file_id,
                source_id,
                chunk.line_start,
                chunk.line_end,
                file.metadata
            ],
        )?;
        if let Some(model_id) = model_id {
            let embedding = &file.vectors[chunk_index];
            let blob = embedding_to_bytes(embedding);
            conn.execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![rowid, blob, embedding.len() as i64, model_id],
            )?;
        }
    }

    // Update file record
    conn.execute(
        "UPDATE knowledge_files SET content_hash = ?1, size_bytes = ?2,
            chunk_count = ?3, indexed_at = datetime('now')
         WHERE id = ?4",
        params![
            file.hash,
            file.content.len() as i64,
            file.chunks.len() as i64,
            file_id
        ],
    )?;

    Ok(file.chunks.len())
}

/// Delete all chunks (FTS5, trigram, embeddings, metadata) for a given file.
fn delete_file_chunks(conn: &rusqlite::Connection, file_id: i64) -> Result<()> {
    let rowids: Vec<i64> = {
        let mut stmt =
            conn.prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| row.get(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // FTS5 tables: delete one at a time (safest for all SQLite versions)
    for rowid in &rowids {
        conn.execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![rowid],
        )?;
        conn.execute(
            "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
            params![rowid],
        )?;
    }

    // Embeddings and metadata (regular tables, bulk delete is fine)
    conn.execute(
        "DELETE FROM knowledge_embeddings WHERE chunk_rowid IN
         (SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1)",
        params![file_id],
    )?;
    conn.execute(
        "DELETE FROM knowledge_chunk_meta WHERE file_id = ?1",
        params![file_id],
    )?;

    Ok(())
}

/// Check if a file is binary by looking for null bytes in the first 1KB.
fn is_binary(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 1024];
    let bytes_read = file.read(&mut buf)?;
    Ok(buf[..bytes_read].contains(&0u8))
}

// ── Wikilink graph resolution ──────────────────────────────────────────────

/// Resolve an Obsidian wikilink target to a file ID.
///
/// Resolution order:
/// 1. Exact match on rel_path
/// 2. Exact match with `.md` appended
/// 3. Basename match (target matches the filename portion of any rel_path)
fn resolve_wikilink(target: &str, files: &HashMap<String, i64>) -> Option<i64> {
    // Exact match
    if let Some(&id) = files.get(target) {
        return Some(id);
    }
    // With .md extension
    let with_md = format!("{}.md", target);
    if let Some(&id) = files.get(&with_md) {
        return Some(id);
    }
    // Basename match: target is just a note name without path
    let suffix_md = format!("/{}.md", target);
    let suffix_plain = format!("/{}", target);
    for (path, &id) in files {
        if path.ends_with(&suffix_md) || path.ends_with(&suffix_plain) {
            return Some(id);
        }
    }
    None
}

/// Delete all wikilink graph edges for a source, then re-resolve from stored
/// metadata. Runs inside the sync transaction after all files are indexed.
///
/// Returns the number of links created.
fn resolve_wikilinks_for_source(conn: &rusqlite::Connection, source_id: i64) -> Result<usize> {
    // Delete existing links for this source's files
    conn.execute(
        "DELETE FROM knowledge_links WHERE source_file_id IN
         (SELECT id FROM knowledge_files WHERE source_id = ?1)",
        params![source_id],
    )?;

    // Load all file rel_paths → IDs for this source
    let files: HashMap<String, i64> = {
        let mut stmt =
            conn.prepare("SELECT id, rel_path FROM knowledge_files WHERE source_id = ?1")?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(0)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    if files.is_empty() {
        return Ok(0);
    }

    // For each file, get one metadata row and extract wikilinks
    let file_wikilinks: Vec<(i64, Vec<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT kf.id, kcm.metadata
             FROM knowledge_files kf
             JOIN knowledge_chunk_meta kcm ON kcm.file_id = kf.id
             WHERE kf.source_id = ?1
             GROUP BY kf.id",
        )?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok())
            .filter_map(|(file_id, meta_json)| {
                let meta: serde_json::Value = serde_json::from_str(&meta_json).ok()?;
                let links = meta.get("wikilinks")?.as_array()?;
                let targets: Vec<String> = links
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if targets.is_empty() {
                    None
                } else {
                    Some((file_id, targets))
                }
            })
            .collect()
    };

    // Resolve and insert links
    let mut links_created = 0usize;
    let mut insert_stmt = conn.prepare(
        "INSERT OR IGNORE INTO knowledge_links
            (source_file_id, target_file_id, link_type, confidence)
         VALUES (?1, ?2, ?3, ?4)",
    )?;

    for (source_file_id, targets) in &file_wikilinks {
        for target in targets {
            if let Some(target_file_id) = resolve_wikilink(target, &files) {
                if target_file_id != *source_file_id {
                    insert_stmt.execute(params![
                        source_file_id,
                        target_file_id,
                        "wikilink",
                        1.0
                    ])?;
                    links_created += 1;
                }
            }
        }
    }

    Ok(links_created)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    use crate::embedder::tests::MockEmbedder;
    use crate::embedder::Embed;

    struct CountingEmbedder {
        calls: Arc<AtomicUsize>,
        embedded_texts: Arc<AtomicUsize>,
    }

    impl Embed for CountingEmbedder {
        fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.embedded_texts.fetch_add(texts.len(), Ordering::SeqCst);
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }

        fn dim(&self) -> usize {
            4
        }
    }

    struct FailingEmbedder;

    impl Embed for FailingEmbedder {
        fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
            anyhow::bail!("synthetic embedding failure")
        }

        fn embed_batch(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            anyhow::bail!("synthetic embedding failure")
        }

        fn dim(&self) -> usize {
            4
        }
    }

    struct MutatingEmbedder {
        file_path: PathBuf,
    }

    impl Embed for MutatingEmbedder {
        fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            fs::write(&self.file_path, "# Concurrent version\nlatest content")?;
            Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
        }

        fn dim(&self) -> usize {
            4
        }
    }

    fn make_store() -> KnowledgeStore {
        crate::knowledge::open_in_memory().expect("in-memory knowledge store")
    }

    fn make_store_with_embedder() -> KnowledgeStore {
        let mut store = make_store();
        store.set_embedder(Arc::new(MockEmbedder::new()));
        store
    }

    fn register_source(store: &KnowledgeStore, dir: &Path) -> KnowledgeSourceInfo {
        store
            .add_source("test", dir.to_str().unwrap(), None, &[])
            .unwrap();
        store.get_source("test").unwrap().unwrap()
    }

    fn register_source_glob(store: &KnowledgeStore, dir: &Path, glob: &str) -> KnowledgeSourceInfo {
        store
            .add_source("test", dir.to_str().unwrap(), Some(glob), &[])
            .unwrap();
        store.get_source("test").unwrap().unwrap()
    }

    fn chunk_count_for_source(store: &KnowledgeStore, source_id: i64) -> i64 {
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
                params![source_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn file_count_for_source(store: &KnowledgeStore, source_id: i64) -> i64 {
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM knowledge_files WHERE source_id = ?1",
                params![source_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    // ── sync_source ─────────────────────────────────────────────────────────

    #[test]
    fn sync_new_source_indexes_all_files() {
        let store = make_store();
        let dir = tempdir().unwrap();
        for i in 0..5 {
            fs::write(
                dir.path().join(format!("file{i}.md")),
                format!("# File {i}\nContent for file {i}"),
            )
            .unwrap();
        }
        let source = register_source(&store, dir.path());
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_added, 5);
        assert_eq!(result.files_unchanged, 0);
        assert_eq!(result.files_updated, 0);
        assert_eq!(result.files_removed, 0);
        assert!(result.chunks_total > 0, "should produce chunks");
        assert_eq!(file_count_for_source(&store, source.id), 5);
    }

    #[test]
    fn sync_unchanged_reports_zero_changes() {
        let store = make_store();
        let dir = tempdir().unwrap();
        for i in 0..5 {
            fs::write(dir.path().join(format!("f{i}.md")), format!("# F{i}\nBody")).unwrap();
        }
        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        let source = store.get_source("test").unwrap().unwrap();
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_unchanged, 5);
        assert_eq!(result.files_added, 0);
        assert_eq!(result.files_updated, 0);
        assert_eq!(result.files_removed, 0);
    }

    #[test]
    fn sync_detects_add_update_remove() {
        let store = make_store();
        let dir = tempdir().unwrap();
        for i in 0..5 {
            fs::write(
                dir.path().join(format!("f{i}.md")),
                format!("# F{i}\nOriginal"),
            )
            .unwrap();
        }
        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        // Modify 2, delete 1, add 1
        fs::write(dir.path().join("f0.md"), "# F0\nModified content").unwrap();
        fs::write(dir.path().join("f1.md"), "# F1\nAlso modified").unwrap();
        fs::remove_file(dir.path().join("f4.md")).unwrap();
        fs::write(dir.path().join("f5.md"), "# F5\nBrand new").unwrap();

        let source = store.get_source("test").unwrap().unwrap();
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_added, 1);
        assert_eq!(result.files_updated, 2);
        assert_eq!(result.files_removed, 1);
        assert_eq!(result.files_unchanged, 2);
        assert_eq!(file_count_for_source(&store, source.id), 5); // 5-1+1 = 5
    }

    #[test]
    fn sync_empty_directory() {
        let store = make_store();
        let dir = tempdir().unwrap();
        let source = register_source(&store, dir.path());
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_scanned, 0);
        assert_eq!(result.files_added, 0);
        assert_eq!(result.chunks_total, 0);
    }

    #[test]
    fn sync_skips_binary_files() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("text.md"), "# Hello\nText").unwrap();
        fs::write(dir.path().join("binary.dat"), b"data\x00\x00\x01").unwrap();

        let source = register_source(&store, dir.path());
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_added, 1, "only text file should be indexed");
    }

    #[test]
    fn sync_invalid_utf8_aborts_without_removing_existing_index() {
        let store = make_store();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("keep.md");
        fs::write(&file_path, "# Kept content\nsearchable phrase").unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();
        let indexed_hash: String = store
            .conn()
            .query_row(
                "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
                params![source.id],
                |row| row.get(0),
            )
            .unwrap();

        fs::write(&file_path, [0xff, 0xfe, 0xfd]).unwrap();
        let result = sync_source(&store, &source, None);

        assert!(result.is_err());
        assert_eq!(file_count_for_source(&store, source.id), 1);
        let retained_hash: String = store
            .conn()
            .query_row(
                "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
                params![source.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained_hash, indexed_hash);
        assert!(chunk_count_for_source(&store, source.id) > 0);
    }

    #[test]
    fn sync_respects_glob_filter() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("notes.md"), "# Notes\nContent").unwrap();
        fs::write(dir.path().join("readme.txt"), "Some text").unwrap();
        fs::write(dir.path().join("code.rs"), "fn main() {}").unwrap();

        let source = register_source_glob(&store, dir.path(), "*.md");
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_added, 1, "only .md files should match");
    }

    // ── reindex_file ────────────────────────────────────────────────────────

    #[test]
    fn reindex_file_skips_unchanged() {
        let store = make_store();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        fs::write(&file_path, "# Test\nContent here").unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        let before = chunk_count_for_source(&store, source.id);
        let source = store.get_source("test").unwrap().unwrap();
        reindex_file(&store, &source, &file_path, None).unwrap();
        let after = chunk_count_for_source(&store, source.id);

        assert_eq!(before, after, "unchanged file should not alter chunks");
    }

    #[test]
    fn reindex_file_updates_changed_content() {
        let store = make_store();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");
        fs::write(&file_path, "# Original\nOld content only").unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        // Modify the file
        fs::write(
            &file_path,
            "# Replacement\nCompletely different text\n\n## Extra\nMore sections",
        )
        .unwrap();

        let source = store.get_source("test").unwrap().unwrap();
        reindex_file(&store, &source, &file_path, None).unwrap();

        // Verify new content is present and old content is gone
        let chunks: Vec<String> = {
            let mut stmt = store
                .conn()
                .prepare(
                    "SELECT kc.content FROM knowledge_chunks kc
                     JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
                     WHERE kcm.source_id = ?1",
                )
                .unwrap();
            stmt.query_map(params![source.id], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        let all = chunks.join("\n");
        assert!(all.contains("Replacement"), "new content should be present");
        assert!(
            !all.contains("Original"),
            "old content should have been replaced"
        );
    }

    #[test]
    fn reindex_file_adds_new_file() {
        let store = make_store();
        let dir = tempdir().unwrap();
        let source = register_source(&store, dir.path());

        let file_path = dir.path().join("new.md");
        fs::write(&file_path, "# Brand New\nFresh content").unwrap();
        reindex_file(&store, &source, &file_path, None).unwrap();

        assert_eq!(file_count_for_source(&store, source.id), 1);
        assert!(chunk_count_for_source(&store, source.id) > 0);
    }

    // ── Embeddings ──────────────────────────────────────────────────────────

    #[test]
    fn sync_generates_embeddings_when_embedder_present() {
        let store = make_store_with_embedder();
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.md"),
            "# Embedding Test\nContent for embedding generation",
        )
        .unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        let count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(count > 0, "embeddings should be generated");
    }

    #[test]
    fn sync_embeds_all_changed_files_in_one_corpus_call_before_persisting() {
        let mut store = make_store();
        let calls = Arc::new(AtomicUsize::new(0));
        let embedded_texts = Arc::new(AtomicUsize::new(0));
        store.set_embedder(Arc::new(CountingEmbedder {
            calls: Arc::clone(&calls),
            embedded_texts: Arc::clone(&embedded_texts),
        }));
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("first.md"), "# First\nfirst document").unwrap();
        fs::write(dir.path().join("second.md"), "# Second\nsecond document").unwrap();
        let source = register_source(&store, dir.path());

        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.embedding_calls, 1);
        assert_eq!(
            result.embedding_chunks,
            embedded_texts.load(Ordering::SeqCst)
        );
        assert_eq!(result.embedding_chunks, result.chunks_indexed);
        assert!(result.transaction_ms <= result.duration_ms);
        let persisted_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(persisted_count as usize, result.embedding_chunks);
    }

    #[test]
    fn embedding_failure_happens_before_the_sync_transaction_and_preserves_index() {
        let mut store = make_store();
        let dir = tempdir().unwrap();
        let existing_path = dir.path().join("existing.md");
        fs::write(&existing_path, "# Original\nExisting indexed content").unwrap();
        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        fs::write(&existing_path, "# Changed\nNew replacement content").unwrap();
        fs::write(dir.path().join("new.md"), "# New file\nNew indexed content").unwrap();
        store.set_embedder(Arc::new(FailingEmbedder));

        let error = sync_source(&store, &source, None).unwrap_err();

        assert!(error
            .to_string()
            .contains("failed to embed changed corpus chunks"));
        assert!(store.conn().is_autocommit());
        assert_eq!(file_count_for_source(&store, source.id), 1);
        assert_eq!(chunk_count_for_source(&store, source.id), 1);
        let retained_content: String = store
            .conn()
            .query_row("SELECT content FROM knowledge_chunks", [], |row| row.get(0))
            .unwrap();
        assert!(retained_content.contains("Existing indexed content"));
        assert!(!retained_content.contains("replacement"));
    }

    #[test]
    fn changed_file_during_embedding_aborts_before_stale_content_is_committed() {
        let mut store = make_store();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("existing.md");
        fs::write(&file_path, "# Original\nExisting indexed content").unwrap();
        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();
        let original_hash: String = store
            .conn()
            .query_row(
                "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
                params![source.id],
                |row| row.get(0),
            )
            .unwrap();

        fs::write(&file_path, "# Prepared\nContent before model inference").unwrap();
        store.set_embedder(Arc::new(MutatingEmbedder {
            file_path: file_path.clone(),
        }));

        let error = sync_source(&store, &source, None).unwrap_err();

        assert!(error
            .to_string()
            .contains("changed while embeddings were being prepared"));
        assert!(store.conn().is_autocommit());
        let stored_hash: String = store
            .conn()
            .query_row(
                "SELECT content_hash FROM knowledge_files WHERE source_id = ?1",
                params![source.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_hash, original_hash);
        let stored_content: String = store
            .conn()
            .query_row("SELECT content FROM knowledge_chunks", [], |row| row.get(0))
            .unwrap();
        assert!(stored_content.contains("Existing indexed content"));
        assert!(!stored_content.contains("Prepared"));
        assert_eq!(file_count_for_source(&store, source.id), 1);
    }

    #[test]
    fn reindex_file_does_not_commit_older_content_after_a_concurrent_edit() {
        let mut store = make_store();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("existing.md");
        fs::write(&file_path, "# Original\nExisting indexed content").unwrap();
        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        fs::write(&file_path, "# Prepared\nContent before model inference").unwrap();
        store.set_embedder(Arc::new(MutatingEmbedder {
            file_path: file_path.clone(),
        }));

        let error = reindex_file(&store, &source, &file_path, None).unwrap_err();

        assert!(error
            .to_string()
            .contains("changed while embeddings were being prepared"));
        assert!(store.conn().is_autocommit());
        let stored_content: String = store
            .conn()
            .query_row("SELECT content FROM knowledge_chunks", [], |row| row.get(0))
            .unwrap();
        assert!(stored_content.contains("Existing indexed content"));
        assert!(!stored_content.contains("Prepared"));
    }

    #[test]
    fn sync_works_without_embedder() {
        let store = make_store(); // no embedder
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.md"), "# No Embed\nStill works").unwrap();

        let source = register_source(&store, dir.path());
        let result = sync_source(&store, &source, None).unwrap();

        assert_eq!(result.files_added, 1);
        let embed_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(embed_count, 0, "no embeddings without embedder");
    }

    // ── FTS5 cleanup ────────────────────────────────────────────────────────

    #[test]
    fn no_orphaned_chunks_after_file_removal() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.md"), "# Keep\nPersistent").unwrap();
        fs::write(dir.path().join("gone.md"), "# Gone\nWill be deleted").unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        fs::remove_file(dir.path().join("gone.md")).unwrap();
        let source = store.get_source("test").unwrap().unwrap();
        sync_source(&store, &source, None).unwrap();

        // No orphaned chunk_meta (meta pointing to deleted files)
        let orphaned: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM knowledge_chunk_meta kcm
                 LEFT JOIN knowledge_files kf ON kcm.file_id = kf.id
                 WHERE kf.id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphaned, 0, "no orphaned chunk_meta");

        // Chunk count in knowledge_chunks matches chunk_meta count
        let fts_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM knowledge_chunks", [], |row| {
                row.get(0)
            })
            .unwrap();
        let meta_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM knowledge_chunk_meta", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fts_count, meta_count, "FTS5 and meta counts should match");
    }

    // ── sync_all ────────────────────────────────────────────────────────────

    #[test]
    fn sync_all_handles_multiple_sources() {
        let store = make_store();
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        fs::write(dir1.path().join("a.md"), "# A\nFirst source").unwrap();
        fs::write(dir2.path().join("b.md"), "# B\nSecond source").unwrap();

        store
            .add_source("src1", dir1.path().to_str().unwrap(), None, &[])
            .unwrap();
        store
            .add_source("src2", dir2.path().to_str().unwrap(), None, &[])
            .unwrap();

        let results = sync_all(&store, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].files_added, 1);
        assert_eq!(results[1].files_added, 1);
    }

    // ── enrichment_fn ───────────────────────────────────────────────────────

    #[test]
    fn enrichment_fn_metadata_stored() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.md"), "# Enriched\nContent").unwrap();
        let source = register_source(&store, dir.path());

        let enrich = |_content: &str, _path: &Path| -> serde_json::Value {
            serde_json::json!({"frontmatter": {"type": "task", "status": "active"}})
        };

        sync_source(&store, &source, Some(&enrich)).unwrap();

        let metadata: String = store
            .conn()
            .query_row(
                "SELECT metadata FROM knowledge_chunk_meta WHERE source_id = ?1 LIMIT 1",
                params![source.id],
                |row| row.get(0),
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(parsed["frontmatter"]["type"], "task");
        assert_eq!(parsed["frontmatter"]["status"], "active");
    }

    // ── walk_source_dir ─────────────────────────────────────────────────────

    #[test]
    fn walk_skips_empty_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("empty.md"), "").unwrap();
        fs::write(dir.path().join("content.md"), "has content").unwrap();

        let files = walk_source_dir(dir.path(), None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("content.md"));
    }

    #[test]
    fn walk_errors_on_nonexistent_path() {
        let result = walk_source_dir(Path::new("/tmp/surely_nonexistent_dir_abc123"), None);
        assert!(result.is_err());
    }

    // ── sha256_hex ──────────────────────────────────────────────────────────

    #[test]
    fn sha256_produces_hex_string() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(hash.len(), 64, "SHA-256 hex is 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_is_deterministic() {
        assert_eq!(sha256_hex(b"same input"), sha256_hex(b"same input"));
    }

    #[test]
    fn sha256_differs_for_different_input() {
        assert_ne!(sha256_hex(b"input a"), sha256_hex(b"input b"));
    }

    // ── resolve_wikilink ────────────────────────────────────────────────────

    #[test]
    fn resolve_exact_path() {
        let files: HashMap<String, i64> = [("01-projects/bpcontext.md".into(), 1)]
            .into_iter()
            .collect();
        assert_eq!(
            resolve_wikilink("01-projects/bpcontext.md", &files),
            Some(1)
        );
    }

    #[test]
    fn resolve_appends_md_extension() {
        let files: HashMap<String, i64> = [("notes/idea.md".into(), 2)].into_iter().collect();
        assert_eq!(resolve_wikilink("notes/idea", &files), Some(2));
    }

    #[test]
    fn resolve_basename_match() {
        let files: HashMap<String, i64> =
            [("deep/path/my-note.md".into(), 3)].into_iter().collect();
        assert_eq!(resolve_wikilink("my-note", &files), Some(3));
    }

    #[test]
    fn resolve_basename_without_extension() {
        let files: HashMap<String, i64> = [("folder/config".into(), 4)].into_iter().collect();
        assert_eq!(resolve_wikilink("config", &files), Some(4));
    }

    #[test]
    fn resolve_returns_none_for_missing() {
        let files: HashMap<String, i64> = [("a.md".into(), 1)].into_iter().collect();
        assert_eq!(resolve_wikilink("nonexistent", &files), None);
    }

    // ── wikilink graph resolution (integration) ─────────────────────────────

    fn register_source_with_enrichments(
        store: &KnowledgeStore,
        dir: &Path,
        enrichments: &[&str],
    ) -> KnowledgeSourceInfo {
        let e: Vec<String> = enrichments.iter().map(|s| s.to_string()).collect();
        store
            .add_source("test", dir.to_str().unwrap(), None, &e)
            .unwrap();
        store.get_source("test").unwrap().unwrap()
    }

    fn link_count(store: &KnowledgeStore, source_id: i64) -> i64 {
        store.link_count_for_source(source_id).unwrap()
    }

    #[test]
    fn sync_creates_wikilink_edges() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "---\ntype: note\n---\nSee [[b]]").unwrap();
        fs::write(dir.path().join("b.md"), "# B\nTarget note").unwrap();

        let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
        let enrich_fn = super::super::enrichment::build_enrichment_fn(
            &source.enrichments,
            Path::new(&source.path),
        );
        let enrich_ref = enrich_fn
            .as_ref()
            .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        let result = sync_source(&store, &source, enrich_ref).unwrap();

        assert!(result.links_created > 0, "should create wikilink edges");
        assert!(link_count(&store, source.id) > 0);
    }

    #[test]
    fn sync_no_self_links() {
        let store = make_store();
        let dir = tempdir().unwrap();
        // File links to itself
        fs::write(dir.path().join("self.md"), "See [[self]]").unwrap();

        let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
        let enrich_fn = super::super::enrichment::build_enrichment_fn(
            &source.enrichments,
            Path::new(&source.path),
        );
        let enrich_ref = enrich_fn
            .as_ref()
            .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        sync_source(&store, &source, enrich_ref).unwrap();

        assert_eq!(
            link_count(&store, source.id),
            0,
            "self-links should be skipped"
        );
    }

    #[test]
    fn sync_unresolvable_links_ignored() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "Link to [[nonexistent]]").unwrap();

        let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
        let enrich_fn = super::super::enrichment::build_enrichment_fn(
            &source.enrichments,
            Path::new(&source.path),
        );
        let enrich_ref = enrich_fn
            .as_ref()
            .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        sync_source(&store, &source, enrich_ref).unwrap();

        assert_eq!(link_count(&store, source.id), 0);
    }

    #[test]
    fn sync_resync_updates_links() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "See [[b]]").unwrap();
        fs::write(dir.path().join("b.md"), "# B").unwrap();
        fs::write(dir.path().join("c.md"), "# C").unwrap();

        let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
        let enrich_fn = super::super::enrichment::build_enrichment_fn(
            &source.enrichments,
            Path::new(&source.path),
        );
        let enrich_ref = enrich_fn
            .as_ref()
            .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        sync_source(&store, &source, enrich_ref).unwrap();
        assert_eq!(link_count(&store, source.id), 1, "a→b");

        // Change a.md to link to c instead
        fs::write(dir.path().join("a.md"), "See [[c]]").unwrap();
        let source = store.get_source("test").unwrap().unwrap();
        let enrich_fn = super::super::enrichment::build_enrichment_fn(
            &source.enrichments,
            Path::new(&source.path),
        );
        let enrich_ref = enrich_fn
            .as_ref()
            .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
        sync_source(&store, &source, enrich_ref).unwrap();

        // Old a→b link should be gone, new a→c should exist
        assert_eq!(link_count(&store, source.id), 1, "a→c only");
    }

    #[test]
    fn sync_without_enrichments_creates_no_links() {
        let store = make_store();
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "See [[b]]").unwrap();
        fs::write(dir.path().join("b.md"), "# B").unwrap();

        let source = register_source(&store, dir.path());
        sync_source(&store, &source, None).unwrap();

        assert_eq!(
            link_count(&store, source.id),
            0,
            "no enrichments = no links"
        );
    }
}
