use crate::embed::Embedder;
use crate::knowledge;
use crate::knowledge::sync::SyncResult;
use crate::knowledge::KnowledgeStore;
use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub(in super::super) struct IndexedCorpus {
    pub(in super::super) store: KnowledgeStore,
    pub(in super::super) sync: SyncResult,
    pub(in super::super) indexing_ms: f64,
}

pub(in super::super) fn index_corpus(
    embedder: Embedder,
    corpus_dir: &Path,
) -> Result<IndexedCorpus> {
    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(embedder));
    store.add_source(
        "retrieval-eval",
        &corpus_dir.to_string_lossy(),
        &[],
        &["frontmatter".into()],
    )?;
    let source = store
        .get_source("retrieval-eval")?
        .ok_or_else(|| anyhow!("could not load evaluation source"))?;
    let enrichment_fn =
        knowledge::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrichment_ref = enrichment_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    let index_start = Instant::now();
    let sync = knowledge::sync::sync_source(&store, &source, enrichment_ref)?;
    let indexing_ms = index_start.elapsed().as_secs_f64() * 1000.0;
    Ok(IndexedCorpus {
        store,
        sync,
        indexing_ms,
    })
}
