use super::{CODE_MODEL, DEFAULT_MODEL, JINA_CODE_MAX_LENGTH, MINI_LM_MAX_LENGTH};
use anyhow::{anyhow, Context, Result};
use fastembed::{EmbeddingModel, TextEmbedding};
use hf_hub::{api::sync::ApiBuilder, Cache};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const TOKENIZER_FILES: [&str; 4] = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

impl super::Embedder {
    /// Download the selected model artifacts explicitly into the local HF cache.
    pub fn download_model(model_dir: &Path, model_name: &str) -> Result<PathBuf> {
        let model = resolve_model(model_name)?;
        let model_info = TextEmbedding::get_model_info(&model)
            .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
        let cache_dir = model_dir.to_path_buf();
        let api = ApiBuilder::new()
            .with_cache_dir(cache_dir.clone())
            .with_progress(true)
            .build()
            .context("failed to initialize Hugging Face cache/API")?;
        let repo = api.model(model_info.model_code.clone());
        let files = std::iter::once(model_info.model_file.as_str())
            .chain(TOKENIZER_FILES.iter().copied())
            .chain(model_info.additional_files.iter().map(String::as_str));
        for file in files {
            repo.get(file).with_context(|| {
                format!("failed to download {file} for {}", model_info.model_code)
            })?;
        }
        Ok(cache_dir)
    }
}

pub(super) fn shared_ort_environment() -> Result<ort::environment::Environment> {
    static ENVIRONMENT: OnceLock<Mutex<Option<ort::environment::Environment>>> = OnceLock::new();
    let environment = ENVIRONMENT.get_or_init(|| Mutex::new(None));
    let mut environment = environment
        .lock()
        .map_err(|_| anyhow!("ONNX Runtime environment lock was poisoned"))?;
    if environment.is_none() {
        *environment = Some(
            ort::init()
                .with_name("bpx-rag")
                .build()
                .map_err(|error| anyhow!("failed to initialize ONNX Runtime: {error}"))?,
        );
    }
    environment
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow!("ONNX Runtime environment initialization failed"))
}

pub fn model_files_present(model_dir: &Path, model_name: &str) -> bool {
    let Ok(model) = resolve_model(model_name) else {
        return false;
    };
    let Ok(model_info) = TextEmbedding::get_model_info(&model) else {
        return false;
    };
    let repo = Cache::new(model_dir.to_path_buf()).model(model_info.model_code.clone());
    std::iter::once(model_info.model_file.as_str())
        .chain(TOKENIZER_FILES.iter().copied())
        .chain(model_info.additional_files.iter().map(String::as_str))
        .all(|file| repo.get(file).is_some())
}

pub fn canonical_model_id(model_name: &str) -> Result<String> {
    let model = resolve_model(model_name)?;
    let info = TextEmbedding::get_model_info(&model)
        .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
    Ok(info.model_code.clone())
}

pub fn model_cache_bytes(model_dir: &Path, model_name: &str) -> Result<u64> {
    let model = resolve_model(model_name)?;
    let info = TextEmbedding::get_model_info(&model)
        .map_err(|error| anyhow!("unsupported embedding model: {error}"))?;
    let repo = Cache::new(model_dir.to_path_buf()).model(info.model_code.clone());
    std::iter::once(info.model_file.as_str())
        .chain(TOKENIZER_FILES.iter().copied())
        .chain(info.additional_files.iter().map(String::as_str))
        .try_fold(0_u64, |total, file| {
            let path = repo
                .get(file)
                .ok_or_else(|| anyhow!("cached model artifact is missing: {file}"))?;
            Ok(total + std::fs::metadata(path)?.len())
        })
}

pub(super) fn resolve_model(model_name: &str) -> Result<EmbeddingModel> {
    match model_name {
        DEFAULT_MODEL | "sentence-transformers/all-MiniLM-L6-v2" => {
            Ok(EmbeddingModel::AllMiniLML6V2)
        }
        CODE_MODEL | "jinaai/jina-embeddings-v2-base-code" => {
            Ok(EmbeddingModel::JinaEmbeddingsV2BaseCode)
        }
        other => anyhow::bail!(
            "unsupported embedding model '{other}'; choose '{DEFAULT_MODEL}' or '{CODE_MODEL}'"
        ),
    }
}

pub(super) fn max_length_for(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::JinaEmbeddingsV2BaseCode => JINA_CODE_MAX_LENGTH,
        _ => MINI_LM_MAX_LENGTH,
    }
}

