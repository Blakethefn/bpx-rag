use super::*;

mod basic;
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

fn indexed_paths(store: &KnowledgeStore) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files ORDER BY rel_path")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}
