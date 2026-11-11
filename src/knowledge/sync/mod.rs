//! Incremental sync engine for knowledge sources.
//!
//! Walks registered source directories, compares content hashes against the
//! database, and re-indexes only changed files.

use crate::chunk::evidence::{EntryObservation, Reference};
use crate::chunk::{chunk_content_for_path, chunker_version_for_path, Symbol};
use crate::embed::Embed;
use crate::knowledge::rebuild;
use crate::knowledge::{scope, KnowledgeSourceInfo, KnowledgeStore};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod files;
mod index;
mod policy;
mod scrub;
use files::*;
use index::*;
use policy::*;
pub use scrub::*;

/// Builds metadata for a file from its content and path. `None` records no enrichment.
pub type EnrichmentFn<'a> = Option<&'a dyn Fn(&str, &Path) -> serde_json::Value>;

/// Result of a knowledge sync operation.
#[derive(Debug, serde::Serialize)]
pub struct SyncResult {
    pub source_label: String,
    pub files_scanned: usize,
    /// Files reached by this scan that were excluded by the sensitive path/content policy.
    pub sensitive_files_skipped: usize,
    /// Files this scan could not index, with the reason: not valid UTF-8, or unreadable. They are not indexed,
    /// and any rows they had are removed. Other files are unaffected.
    pub skipped_files: Vec<SkippedFile>,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_unchanged: usize,
    pub chunks_total: usize,
    /// Chunks prepared and written by this sync operation.
    pub chunks_indexed: usize,
    /// Chunks produced by syntax-aware Tree-sitter grammars during this sync.
    pub tree_sitter_chunks_indexed: usize,
    /// Chunks produced by the existing fallback chunker during this sync.
    pub existing_chunker_chunks_indexed: usize,
    pub tree_sitter_files_indexed: usize,
    pub existing_chunker_files_indexed: usize,
    pub links_created: usize,
    /// Time spent waiting for and running the embedding model, in milliseconds.
    pub embedding_ms: u64,
    /// Number of configured-size embedding batches made through the Embed trait.
    pub embedding_calls: usize,
    /// Number of chunk texts embedded before writing this sync transaction.
    pub embedding_chunks: usize,
    /// Time spent in the SQLite write transaction, in milliseconds.
    pub transaction_ms: u64,
    pub duration_ms: u64,
}

/// A file that a sync read but did not index, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedFile {
    pub rel_path: String,
    pub reason: String,
}

/// Sync a single knowledge source. Walks the directory, hashes files,
/// and re-indexes only changed/new files. Removes chunks for deleted files.
/// Prepared-file edits are revalidated after embedding; edits to unselected
/// files during the walk are picked up by the next sync.
pub fn sync_source(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: EnrichmentFn<'_>,
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let base_path = Path::new(&source.path);

    // 1. Walk directory (respects .gitignore and skips binary/empty/sensitive files)
    let walk = walk_source_dir_with_report(base_path, &source.patterns)?;
    let disk_files = walk.files;
    let mut sensitive_files_skipped = walk.sensitive_files_skipped;

    // 2. Load existing file records from DB
    let db_files = load_db_files(conn, source.id)?;

    // 3. Classify every file on disk. Indexed files that are no longer selected are removed.
    let mut plan = ChangePlan::default();
    let mut excluded_content_paths = HashSet::new();
    // Files the walk could not open are skipped. They are not in `disk_files`, so any rows
    // they had are removed below.
    let mut skipped = walk.unreadable;
    for (rel_path, abs_path) in &disk_files {
        match classify_file(rel_path, abs_path, &db_files, &mut plan)? {
            Classification::Indexed => {}
            Classification::Sensitive => {
                sensitive_files_skipped += 1;
                excluded_content_paths.insert(rel_path.clone());
            }
            Classification::Skipped(reason) => {
                skipped.push(SkippedFile {
                    rel_path: rel_path.clone(),
                    reason,
                });
                excluded_content_paths.insert(rel_path.clone());
            }
        }
    }
    for (rel_path, db_file) in &db_files {
        if !disk_files.contains_key(rel_path) || excluded_content_paths.contains(rel_path) {
            plan.to_remove.push(db_file.id);
        }
    }

    let counts = ScanCounts {
        files_scanned: disk_files.len(),
        sensitive_files_skipped,
        skipped,
    };
    apply_plan(store, source, &db_files, plan, enrichment_fn, start, counts)
}

/// Reindex only the listed relative paths, under the same policy as `sync_source`. A listed file
/// that is included is reindexed when its content changed. A listed file that is excluded,
/// sensitive, or gone has its rows removed. Other files are not read or hashed.
pub fn sync_paths(
    store: &KnowledgeStore,
    source: &KnowledgeSourceInfo,
    enrichment_fn: EnrichmentFn<'_>,
    rel_paths: &[String],
) -> Result<SyncResult> {
    let start = Instant::now();
    let conn = store.conn();
    let base_path = Path::new(&source.path);

    // The walk decides which paths the policy includes. It reads directory entries and sniffs
    // binary content, but it hashes nothing.
    let walk = walk_source_dir_with_report(base_path, &source.patterns)?;
    let files_scanned = walk.files.len();
    let mut sensitive_files_skipped = walk.sensitive_files_skipped;
    let db_files = load_db_files(conn, source.id)?;

    let mut plan = ChangePlan::default();
    let unique_paths: BTreeSet<&String> = rel_paths.iter().collect();
    // Report only the unreadable files that were asked about. A listed file the walk could
    // not open is absent from `walk.files`, so its rows are removed below.
    let mut skipped: Vec<SkippedFile> = walk
        .unreadable
        .iter()
        .filter(|file| unique_paths.contains(&file.rel_path))
        .cloned()
        .collect();
    for rel_path in unique_paths {
        match walk.files.get(rel_path) {
            Some(abs_path) => match classify_file(rel_path, abs_path, &db_files, &mut plan)? {
                Classification::Indexed => {}
                Classification::Sensitive => {
                    sensitive_files_skipped += 1;
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
                Classification::Skipped(reason) => {
                    skipped.push(SkippedFile {
                        rel_path: rel_path.clone(),
                        reason,
                    });
                    mark_excluded(&db_files, rel_path, &mut plan);
                }
            },
            None => mark_excluded(&db_files, rel_path, &mut plan),
        }
    }

    let counts = ScanCounts {
        files_scanned,
        sensitive_files_skipped,
        skipped,
    };
    apply_plan(store, source, &db_files, plan, enrichment_fn, start, counts)
}

/// What the walk saw, reported in the sync result.
struct ScanCounts {
    files_scanned: usize,
    sensitive_files_skipped: usize,
    skipped: Vec<SkippedFile>,
}

/// Changes decided by classification, applied in one transaction.
#[derive(Default)]
struct ChangePlan {
    /// (relative path, absolute path, content hash, stat)
    to_add: Vec<(String, PathBuf, String, StatRecord)>,
    /// (file id, absolute path, content hash, stat)
    to_update: Vec<(i64, PathBuf, String, StatRecord)>,
    to_remove: Vec<i64>,
    /// Unchanged files whose symbol or reference stamp is stale. Evidence comes from the content
    /// read during classification, not from a later disk read.
    backfill: Vec<BackfillEvidence>,
    /// Unchanged files whose modification time was never recorded.
    backfill_stat: Vec<(i64, StatRecord)>,
    unchanged: usize,
}

struct BackfillEvidence {
    file_id: i64,
    rel_path: String,
    status: &'static str,
    symbols: Vec<Symbol>,
    references: Vec<Reference>,
    entries: Vec<EntryObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Classification {
    Indexed,
    Sensitive,
    /// Not valid UTF-8. The file is skipped with this reason, and any rows it had are removed.
    Skipped(String),
}

/// A file that could not be read, with the operating system's reason.
fn unreadable_file(rel_path: &str, error: &dyn std::fmt::Display) -> SkippedFile {
    SkippedFile {
        rel_path: rel_path.to_string(),
        reason: format!("unreadable: {error}"),
    }
}

/// Record a file that was not indexed and must lose any rows it had.
fn mark_excluded(db_files: &HashMap<String, DbFile>, rel_path: &str, plan: &mut ChangePlan) {
    if let Some(db_file) = db_files.get(rel_path) {
        plan.to_remove.push(db_file.id);
    }
}

/// Read one included file, decide whether it is new, changed, or unchanged, and record any
/// backfill it needs. The stat is taken before the content is read.
fn classify_file(
    rel_path: &str,
    abs_path: &Path,
    db_files: &HashMap<String, DbFile>,
    plan: &mut ChangePlan,
) -> Result<Classification> {
    // A file that cannot be inspected or read (permission denied, an I/O error, or removed
    // since the walk) is a skip with a reason, like invalid UTF-8, so one bad file cannot stop
    // the other files from syncing. Its indexed rows are removed, because the index could no
    // longer be checked against the file.
    let stat = match stat_before_read(abs_path) {
        Ok(stat) => stat,
        Err(error) => {
            return Ok(Classification::Skipped(format!(
                "unreadable: {}",
                error.root_cause()
            )))
        }
    };
    let bytes = match fs::read(abs_path) {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Classification::Skipped(format!("unreadable: {error}"))),
    };

    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) => return Ok(Classification::Skipped(format!("not valid UTF-8: {error}"))),
    };
    if contains_google_api_key(&content) {
        return Ok(Classification::Sensitive);
    }
    let hash = sha256_hex(content.as_bytes());

    let chunker_version = chunker_version_for_path(Path::new(rel_path));
    match db_files.get(rel_path) {
        Some(db_file) if db_file.hash != hash || db_file.chunker_version != chunker_version => {
            plan.to_update
                .push((db_file.id, abs_path.to_path_buf(), hash, stat));
        }
        Some(db_file) => {
            plan.unchanged += 1;
            // The content matches, but its symbol or reference rows may come from an older
            // writer. `evidence_current` compares both stamps with the stored hash.
            if !db_file.evidence_current {
                let evidence = chunk_content_for_path(Path::new(rel_path), &content);
                plan.backfill.push(BackfillEvidence {
                    file_id: db_file.id,
                    rel_path: rel_path.to_string(),
                    status: evidence.symbol_status,
                    symbols: evidence.symbols,
                    references: evidence.references,
                    entries: evidence.entries,
                });
            }
            if !db_file.stat_recorded {
                plan.backfill_stat.push((db_file.id, stat));
            }
        }
        None => plan
            .to_add
            .push((rel_path.to_string(), abs_path.to_path_buf(), hash, stat)),
    }
    Ok(Classification::Indexed)
}

/// Prepare changed files, embed them, and apply the plan in one transaction.
fn apply_plan(
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

    // Prepare changed files before acquiring SQLite's write lock. Re-read only
    // files selected for indexing and verify they still match the classification
    // hash, keeping source text out of the corpus-wide staging vectors.
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

/// Embed every chunk of the prepared files in batches. Returns the number of model calls.
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

/// Apply the plan inside one `BEGIN IMMEDIATE` transaction. Returns the links created and the
/// transaction time in milliseconds.
fn commit_plan(
    conn: &rusqlite::Connection,
    source: &KnowledgeSourceInfo,
    db_files: &HashMap<String, DbFile>,
    plan: &ChangePlan,
    prepared_files: &[PreparedFile],
    model_id: Option<&str>,
    changed: bool,
) -> Result<(usize, u64)> {
    let transaction_start = Instant::now();
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let inner = (|| -> Result<usize> {
        // Another process may have synced/reindexed this source while model
        // inference ran. Refuse to apply work based on a stale file-row map.
        if &load_db_files(conn, source.id)? != db_files {
            anyhow::bail!("knowledge source changed during sync preparation; retry sync");
        }

        // 4a. Remove deleted files
        for file_id in &plan.to_remove {
            delete_file_chunks(conn, *file_id)?;
            conn.execute(
                "DELETE FROM knowledge_files WHERE id = ?1",
                params![file_id],
            )?;
        }

        // 4b. Backfill symbols, references, and entries for unchanged files. Chunks,
        // hashes, and vectors are not touched. The content hash is already current, so the
        // stamp records that same hash.
        for file in &plan.backfill {
            replace_file_evidence(
                conn,
                source.id,
                file.file_id,
                &file.rel_path,
                file.status,
                &file.symbols,
                &file.references,
                &file.entries,
            )?;
        }
        for (file_id, stat) in &plan.backfill_stat {
            conn.execute(
                "UPDATE knowledge_files SET mtime_ns = ?1, stat_checked_ns = ?2 WHERE id = ?3",
                params![stat.mtime_ns, stat.checked_ns, file_id],
            )?;
        }

        // 4c. Replace changed files and add new files using prepared vectors.
        for file in prepared_files {
            let file_id = match file.file_id {
                Some(file_id) => {
                    delete_file_chunks(conn, file_id)?;
                    file_id
                }
                None => {
                    conn.execute(
                        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![source.id, file.rel_path, file.hash, file.size_bytes],
                    )?;
                    conn.last_insert_rowid()
                }
            };
            index_prepared_file(conn, source.id, file_id, file, model_id)?;
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

        // 4d-2. Index identifier words for chunks that older writers queued. Current writers
        // handle their own chunks, so this only does work for legacy inserts.
        crate::knowledge::identifiers::index_pending(conn)?;

        // 4e. Resolve wikilinks and code relations to graph edges. Code edges
        // depend on the whole indexed set, so they are rebuilt only when this
        // sync changed something or the source has none yet.
        let lc = resolve_wikilinks_for_source(conn, source.id)?;
        let has_code_edges: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM knowledge_links kl
             JOIN knowledge_files kf ON kf.id = kl.source_file_id
             WHERE kf.source_id = ?1 AND kl.link_type LIKE 'code:%')",
            params![source.id],
            |row| row.get(0),
        )?;
        if changed || !has_code_edges {
            let root = Path::new(&source.path).canonicalize()?;
            resolve_code_edges(conn, source.id, &root)?;
        }

        // Findings are derived from stored evidence. The `changed` flag is the wrong gate:
        // a symbol or reference backfill rewrites evidence while that flag stays false.
        // A missing or outdated analysis row is rebuilt inside `rebuild_source` even when
        // this flag is false.
        let structural = !plan.to_add.is_empty()
            || !plan.to_update.is_empty()
            || !plan.to_remove.is_empty()
            || !plan.backfill.is_empty();
        // Implicit Cargo roots follow the indexed paths, including a file this sync just added.
        // Cargo.toml itself is not re-chunked when only that file changed.
        if structural {
            crate::knowledge::entries::refresh_implicit(conn, source.id, Path::new(&source.path))?;
        }
        rebuild::rebuild_source(conn, source.id, structural)?;

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
    Ok((
        links_created,
        transaction_start.elapsed().as_millis() as u64,
    ))
}

/// Walk a knowledge source directory, returning files keyed by relative path.
/// Respects .gitignore, skips binary/empty/sensitive files, and applies the glob filter.
/// A traversal error aborts the sync so reconciliation cannot mistake an incomplete scan
/// for deleted files. A file that cannot be opened or inspected is left out of `files`, and
/// `walk_source_dir_with_report` names it with the reason.
pub(crate) fn walk_source_dir(
    base_path: &Path,
    patterns: &[String],
) -> Result<BTreeMap<String, PathBuf>> {
    Ok(walk_source_dir_with_report(base_path, patterns)?.files)
}

/// Files keyed by relative path. A `BTreeMap` keeps sync order (and so chunk
/// and file rowids, which tie-break search results) the same across runs.
pub(crate) struct SourceWalkReport {
    pub files: BTreeMap<String, PathBuf>,
    pub sensitive_files_skipped: usize,
    /// Files the walk could not open or inspect, keyed like `files`, with the reason.
    pub unreadable: Vec<SkippedFile>,
}

pub(crate) fn walk_source_dir_with_report(
    base_path: &Path,
    patterns: &[String],
) -> Result<SourceWalkReport> {
    if !base_path.is_dir() {
        return Err(anyhow::anyhow!(
            "knowledge source path is not a directory: {}",
            base_path.display()
        ));
    }
    let canonical_root = base_path
        .canonicalize()
        .with_context(|| format!("could not resolve knowledge source {}", base_path.display()))?;
    if is_sensitive_source_root(&canonical_root) {
        anyhow::bail!("refusing to index a local credential-store directory");
    }

    let mut builder = WalkBuilder::new(base_path);
    builder
        .hidden(false) // Don't skip hidden files (dotfiles may be relevant)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    if let Some(overrides) = scope::build_overrides(base_path, patterns)? {
        builder.overrides(overrides);
    }

    let mut files = BTreeMap::new();
    let mut sensitive_files_skipped = 0;
    let mut unreadable = Vec::new();

    for entry in builder.build() {
        let entry = entry.context("failed while traversing knowledge source")?;

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let abs_path = entry.path().to_path_buf();
        if is_sensitive_path(base_path, &abs_path) {
            sensitive_files_skipped += 1;
            continue;
        }

        let rel_path = abs_path
            .strip_prefix(base_path)
            .unwrap_or(&abs_path)
            .to_string_lossy()
            .to_string();

        // A file that cannot be opened or inspected is skipped with a reason. Only a failure
        // of the traversal itself (above) is fatal, because it hides what a directory holds.
        // Skip binary files (null byte in first 1KB)
        match is_binary(&abs_path) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                unreadable.push(unreadable_file(&rel_path, &error));
                continue;
            }
        }

        // Skip empty files
        match fs::metadata(&abs_path) {
            Ok(metadata) if metadata.len() == 0 => continue,
            Ok(_) => {}
            Err(error) => {
                unreadable.push(unreadable_file(&rel_path, &error));
                continue;
            }
        }

        files.insert(rel_path, abs_path);
    }

    Ok(SourceWalkReport {
        files,
        sensitive_files_skipped,
        unreadable,
    })
}

#[cfg(test)]
mod tests;
