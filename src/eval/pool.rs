use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod channel;
mod generate;
mod import;
mod output;

#[cfg(test)]
mod tests;

pub(in crate::eval) use channel::PoolChannel;
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
