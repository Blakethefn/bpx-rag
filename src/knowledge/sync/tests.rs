use super::*;

mod clean;
mod core;
mod edges;
mod embed;
mod identifier;
mod paths;
mod scrub;
mod stamp;
mod unreadable;
mod upgrade;
mod utf8;
mod wikilink;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;

use crate::embed::Embed;
use crate::support::{add_source, make_store};

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

#[test]
fn enrichment_fn_metadata_stored() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.md"), "# Enriched\nContent").unwrap();
    let source = add_source(&store, "test", dir.path());

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
