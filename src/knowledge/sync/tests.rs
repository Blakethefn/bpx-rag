use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

use crate::embed::tests::MockEmbedder;
use crate::embed::Embed;

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
        .add_source("test", dir.to_str().unwrap(), &[], &[])
        .unwrap();
    store.get_source("test").unwrap().unwrap()
}

fn register_source_glob(store: &KnowledgeStore, dir: &Path, glob: &str) -> KnowledgeSourceInfo {
    store
        .add_source("test", dir.to_str().unwrap(), &[glob.to_string()], &[])
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
fn sync_assigns_file_ids_in_path_order() {
    // Rowids break ties in search results, so insert order must not depend on
    // the directory walk or on hash-map iteration.
    let store = make_store();
    let dir = tempdir().unwrap();
    for name in ["zeta.md", "alpha.md", "mid.md", "beta.md"] {
        fs::write(dir.path().join(name), format!("content of {name}")).unwrap();
    }
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    let mut stmt = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files WHERE source_id = ?1 ORDER BY id")
        .unwrap();
    let ordered: Vec<String> = stmt
        .query_map(params![source.id], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(ordered, ["alpha.md", "beta.md", "mid.md", "zeta.md"]);
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

    let files = walk_source_dir(dir.path(), &[]).unwrap();
    assert_eq!(files.len(), 1);
    assert!(files.contains_key("content.md"));
}

#[test]
fn walk_errors_on_nonexistent_path() {
    let result = walk_source_dir(Path::new("/tmp/surely_nonexistent_dir_abc123"), &[]);
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
    let files: HashMap<String, i64> = [("deep/path/my-note.md".into(), 3)].into_iter().collect();
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
        .add_source("test", dir.to_str().unwrap(), &[], &e)
        .unwrap();
    store.get_source("test").unwrap().unwrap()
}

fn link_count(store: &KnowledgeStore, source_id: i64) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_links
             WHERE source_file_id IN (SELECT id FROM knowledge_files WHERE source_id = ?1)",
            rusqlite::params![source_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn sync_creates_wikilink_edges() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "---\ntype: note\n---\nSee [[b]]").unwrap();
    fs::write(dir.path().join("b.md"), "# B\nTarget note").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn =
        super::super::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
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
    let enrich_fn =
        super::super::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
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
    let enrich_fn =
        super::super::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
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
    let enrich_fn =
        super::super::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync_source(&store, &source, enrich_ref).unwrap();
    assert_eq!(link_count(&store, source.id), 1, "a→b");

    // Change a.md to link to c instead
    fs::write(dir.path().join("a.md"), "See [[c]]").unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    let enrich_fn =
        super::super::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
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

fn code_edge_count(store: &KnowledgeStore, source_id: i64) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_links kl
             JOIN knowledge_files kf ON kf.id = kl.source_file_id
             WHERE kf.source_id = ?1 AND kl.link_type LIKE 'code:%'",
            params![source_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn sync_stores_rust_module_relations_and_drops_them_when_removed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "mod util;\npub fn run() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/util.rs"),
        "pub fn helper() -> u8 { 1 }\n",
    )
    .unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    let lib_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = 'src/lib.rs'",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    let related = store.related_files(lib_id, 10).unwrap();
    assert_eq!(related, vec!["src/util.rs".to_string()]);
    assert!(code_edge_count(&store, source.id) >= 1);

    fs::write(dir.path().join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    sync_source(&store, &source, None).unwrap();
    assert!(
        store.related_files(lib_id, 10).unwrap().is_empty(),
        "edge is dropped once lib.rs no longer declares the module"
    );
}

#[test]
fn unchanged_sync_keeps_code_edges_without_rereading_files() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "mod util;\n").unwrap();
    fs::write(dir.path().join("util.rs"), "pub fn helper() {}\n").unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();
    let edges = code_edge_count(&store, source.id);
    assert!(edges >= 1);

    let second = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        second.files_added + second.files_updated + second.files_removed,
        0
    );
    assert_eq!(code_edge_count(&store, source.id), edges);
}

fn indexed_rel_paths(store: &KnowledgeStore, source_id: i64) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files WHERE source_id = ?1 ORDER BY rel_path")
        .unwrap();
    stmt.query_map(params![source_id], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

#[test]
fn sync_never_indexes_credential_files_or_key_material() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("notes")).unwrap();
    fs::create_dir_all(dir.path().join("secrets")).unwrap();
    fs::create_dir_all(dir.path().join(".ssh")).unwrap();
    fs::write(
        dir.path().join("notes/readme.md"),
        "# Readme\nkestrel public notes",
    )
    .unwrap();
    fs::write(dir.path().join(".env"), "DATABASE_URL=kestrel-private").unwrap();
    fs::write(dir.path().join("secrets/token.txt"), "kestrel token value").unwrap();
    fs::write(dir.path().join(".ssh/config"), "Host kestrel-bastion").unwrap();
    fs::write(dir.path().join("server.pem"), "-----BEGIN PRIVATE KEY-----").unwrap();
    // Built at runtime so the repository never contains a key-shaped literal.
    let fake_key = format!("AIza{}", "0123456789abcdefghijklmnopqrstuvwxy");
    fs::write(
        dir.path().join("leak.md"),
        format!("# Leak\nkestrel key {fake_key}"),
    )
    .unwrap();
    let source = register_source(&store, dir.path());

    sync_source(&store, &source, None).unwrap();

    assert_eq!(
        indexed_rel_paths(&store, source.id),
        vec!["notes/readme.md".to_string()]
    );
    let leaked: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunks WHERE content LIKE '%kestrel-private%' OR content LIKE '%kestrel token%' OR content LIKE '%kestrel-bastion%' OR content LIKE '%AIza%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0, "no chunk holds credential text");
}

#[test]
fn scrub_removes_rows_that_match_the_policy_after_they_were_indexed() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("readme.md"), "# Readme\nkestrel public").unwrap();
    let source = register_source(&store, dir.path());
    sync_source(&store, &source, None).unwrap();

    // Simulate a row written before the policy existed.
    let conn = store.conn();
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, '.env', 'legacy', 32)",
        params![source.id],
    )
    .unwrap();
    let file_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('.env', 'DATABASE_URL=kestrel-legacy')",
        [],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
         VALUES (?1, ?2, ?3, 1, 1, '{}')",
        params![rowid, file_id, source.id],
    )
    .unwrap();

    scrub_sensitive_indexed_files(&store, Some(source.id)).unwrap();

    assert_eq!(
        indexed_rel_paths(&store, source.id),
        vec!["readme.md".to_string()]
    );
    let chunks_left: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunks WHERE content LIKE '%kestrel-legacy%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        chunks_left, 0,
        "scrub removes the chunks as well as the file row"
    );
}

#[test]
fn wikilink_resolution_reports_malformed_chunk_metadata() {
    let store = crate::knowledge::open_in_memory().unwrap();
    let source_id = store.add_source("vault", "/vault", &[], &[]).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
             VALUES (?1, 'note.md', 'hash', 1)",
            params![source_id],
        )
        .unwrap();
    let file_id = store.conn().last_insert_rowid();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
             VALUES (1, ?1, ?2, 1, 1, '{not json')",
            params![file_id, source_id],
        )
        .unwrap();

    let result = super::index::resolve_wikilinks_for_source(store.conn(), source_id);

    assert!(
        result.is_err(),
        "malformed metadata must not drop wikilinks silently"
    );
}

// ── identifier index upgrade ─────────────────────────────────────────────

#[test]
fn upgrading_the_identifier_index_does_not_reembed_unchanged_files() {
    let mut store = make_store();
    let calls = Arc::new(AtomicUsize::new(0));
    let embedded_texts = Arc::new(AtomicUsize::new(0));
    store.set_embedder(Arc::new(CountingEmbedder {
        calls: Arc::clone(&calls),
        embedded_texts: Arc::clone(&embedded_texts),
    }));
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("users.ts"),
        "export function getUserById(id) { return id; }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("notes.md"),
        "# Notes\nblue-green-deploy plan\n",
    )
    .unwrap();
    let source = register_source(&store, dir.path());
    let first = sync_source(&store, &source, None).unwrap();
    assert_eq!(first.files_added, 2);
    let embed_calls_after_index = calls.load(Ordering::SeqCst);

    // Simulate a database from before the identifier index existed.
    store
        .conn()
        .execute_batch("DELETE FROM knowledge_identifiers; DROP TABLE knowledge_index_meta;")
        .unwrap();
    crate::knowledge::schema::apply_schema(store.conn()).unwrap();

    let second = sync_source(&store, &source, None).unwrap();

    assert_eq!(
        second.files_updated, 0,
        "unchanged files must not be reindexed"
    );
    assert_eq!(second.files_unchanged, 2);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        embed_calls_after_index,
        "no embedding call may be made for an identifier-index upgrade"
    );
    let identifier_rows: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_identifiers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        identifier_rows, 2,
        "backfill must restore one row per chunk"
    );
}
