use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

mod generate;
mod import;

pub use generate::generate_judgment_pool;
pub use import::import_judgment_pool;

pub(super) const JUDGMENT_POOL_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolArtifact {
    pub schema_version: u8,
    pub seed: u64,
    pub candidate_depth_per_retriever: usize,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
    pub corpus_file_count: usize,
    pub chunk_count: usize,
    pub grade_guide: Vec<GradeGuideEntry>,
    pub candidates: Vec<JudgmentPoolCandidate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolCandidate {
    pub blinded_query_id: String,
    pub query: String,
    pub candidate_id: String,
    pub path: String,
    pub evidence: Vec<JudgmentPoolEvidence>,
    pub grade: Option<u8>,
    pub reviewer_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GradeGuideEntry {
    pub grade: u8,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentPoolEvidence {
    pub title: String,
    pub lines: String,
    pub snippet: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolKey {
    pub schema_version: u8,
    pub seed: u64,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
    pub model_id: String,
    pub candidate_depth_per_retriever: usize,
    pub candidates: Vec<JudgmentPoolKeyEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JudgmentPoolKeyEntry {
    pub candidate_id: String,
    pub blinded_query_id: String,
    pub query_id: String,
    pub path: String,
    pub retrieved_by: PoolRetrieverRanks,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolRetrieverRanks {
    pub hybrid: Option<usize>,
    pub lexical: Option<usize>,
    pub vector: Option<usize>,
}

#[derive(Default)]
pub(super) struct PoolCandidateAccumulator {
    pub(super) evidence: BTreeMap<String, JudgmentPoolEvidence>,
    pub(super) retrieved_by: PoolRetrieverRanks,
}

#[derive(Debug, Serialize)]
pub struct JudgmentPoolGenerationReport {
    pub candidate_count: usize,
    pub query_count: usize,
    pub corpus_file_count: usize,
    pub chunk_count: usize,
    pub pool_output: String,
    pub key_output: String,
    pub dataset_queries_sha256: String,
    pub corpus_manifest_sha256: String,
}

pub(super) fn opaque_id(seed: u64, kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_be_bytes());
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{kind}-{}", &digest[..20])
}

pub(super) fn normalized_output_target(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path must include a file name"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        anyhow::bail!("output directory does not exist: {}", parent.display());
    }
    Ok(parent.canonicalize()?.join(file_name))
}

pub(super) fn ensure_new_output(path: &Path) -> Result<()> {
    let _ = normalized_output_target(path)?;
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing output {}", path.display());
    }
    Ok(())
}

pub(super) fn ensure_new_distinct_outputs(first: &Path, second: &Path) -> Result<()> {
    ensure_new_output(first)?;
    ensure_new_output(second)?;
    if normalized_output_target(first)? == normalized_output_target(second)? {
        anyhow::bail!("pool and reveal-key outputs must be different files");
    }
    Ok(())
}

pub(super) fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

pub(super) fn write_new_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

pub(super) fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON file {}", path.display()))
}
