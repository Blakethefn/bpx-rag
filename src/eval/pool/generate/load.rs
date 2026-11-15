use super::super::super::{corpus_manifest_sha256, parse_jsonl, validate_queries, EvalQuery};
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use crate::knowledge::KnowledgeStore;
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) struct LoadedPool {
    pub(super) model_id: String,
    pub(super) queries: Vec<EvalQuery>,
    pub(super) query_path: PathBuf,
    pub(super) dataset_queries_sha256: String,
    pub(super) corpus_dir: PathBuf,
    pub(super) corpus_manifest_sha256: String,
    pub(super) store: KnowledgeStore,
    pub(super) source_id: i64,
    pub(super) files_scanned: usize,
    pub(super) chunks_total: usize,
}

pub(super) fn load_inputs(
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
) -> Result<LoadedPool> {
    let config = Config::load()?;
    let model_dir = crate::app::model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        anyhow::bail!(
            "judgment pooling requires the configured model to be cached; run `bpx-rag models download` first ({})",
            model_dir.display()
        );
    }
    let model = Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
    )?;
    let model_id = model.model_id().to_string();

    let data_dir = dataset.map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/retrieval"),
        Path::to_path_buf,
    );
    let corpus_dir = corpus_root.map_or_else(|| data_dir.join("corpus"), Path::to_path_buf);
    if !data_dir.is_dir() {
        anyhow::bail!(
            "judgment dataset directory does not exist: {}",
            data_dir.display()
        );
    }
    if !corpus_dir.is_dir() {
        anyhow::bail!(
            "judgment corpus directory does not exist: {}",
            corpus_dir.display()
        );
    }
    let query_path = data_dir.join("queries.jsonl");
    let query_bytes = fs::read(&query_path)
        .with_context(|| format!("failed to read evaluation queries {}", query_path.display()))?;
    let dataset_queries_sha256 = format!("{:x}", Sha256::digest(&query_bytes));
    let query_contents = String::from_utf8(query_bytes)
        .with_context(|| format!("evaluation queries are not UTF-8: {}", query_path.display()))?;
    let queries = parse_jsonl::<EvalQuery>(&query_contents, &query_path)?;
    validate_queries(&queries)?;
    if queries.is_empty() {
        anyhow::bail!("judgment query set is empty");
    }
    let corpus_before = corpus_manifest_sha256(&corpus_dir)?;

    let mut store = knowledge::open_in_memory()?;
    store.set_embedder(Arc::new(model));
    store.add_source(
        "judgment-pool",
        &corpus_dir.to_string_lossy(),
        &[],
        &["frontmatter".into()],
    )?;
    let source = store
        .get_source("judgment-pool")?
        .ok_or_else(|| anyhow!("could not load judgment-pool source"))?;
    let enrichment =
        knowledge::enrichment::build_enrichment_fn(&source.enrichments, Path::new(&source.path));
    let enrichment_ref = enrichment
        .as_ref()
        .map(|function| function.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    let sync = knowledge::sync::sync_source(&store, &source, enrichment_ref)?;
    let corpus_after = corpus_manifest_sha256(&corpus_dir)?;
    if corpus_before != corpus_after {
        anyhow::bail!("judgment corpus changed during indexing; retry pool generation");
    }
    Ok(LoadedPool {
        model_id,
        queries,
        query_path,
        dataset_queries_sha256,
        corpus_dir,
        corpus_manifest_sha256: corpus_after,
        source_id: source.id,
        files_scanned: sync.files_scanned,
        chunks_total: sync.chunks_total,
        store,
    })
}
