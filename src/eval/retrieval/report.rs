use crate::knowledge::query::{LayerPolicy, LayerReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswerability {
    #[default]
    Answerable,
    UnanswerableInCorpus,
}

macro_rules! metric_record {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        metric_record! {
            @record
            $(#[$meta])*
            $vis struct $name {}
            rates {
                $($(#[$field_meta])* $field_vis $field: f64),*
            }
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($held:tt)*
        }
        rates {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        metric_record! {
            @record
            $(#[$meta])*
            $vis struct $name { $($held)* }
            rates {
                $($(#[$field_meta])* $field_vis $field: f64),*
            }
        }
    };
    (
        @record
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($held:tt)*
        }
        rates {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: f64),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($held)*
            $(
                $(#[$field_meta])*
                $field_vis $field: f64,
            )*
        }

        impl $name {
            pub(in crate::eval) fn metric_values(&self) -> impl Iterator<Item = f64> + '_ {
                [$(self.$field),*].into_iter()
            }

            fn metric_values_mut(&mut self) -> impl Iterator<Item = &mut f64> {
                [$( &mut self.$field ),*].into_iter()
            }

            pub(in crate::eval) fn accumulate(&mut self, sample: &Self) {
                for (total, value) in self.metric_values_mut().zip(sample.metric_values()) {
                    *total += value;
                }
            }

            pub(in crate::eval) fn mean(&mut self, count: usize) {
                let divisor = count.max(1) as f64;
                for value in self.metric_values_mut() {
                    *value /= divisor;
                }
            }
        }
    };
}

metric_record! {
    #[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
    pub struct QualityMetrics {
        pub recall_at_5: f64,
        pub recall_at_10: f64,
        pub precision_at_5: f64,
        pub precision_at_10: f64,
        pub judged_coverage_at_5: f64,
        pub judged_coverage_at_10: f64,
        pub mrr_at_10: f64,
        pub ndcg_at_10: f64,
        pub success_at_5: f64,
    }
}

#[derive(Debug, Serialize)]
pub struct RetrievalEvalReport {
    pub model_name: String,
    pub model_id: String,
    pub embedding_device: String,
    pub layer_policy: LayerPolicy,
    pub dataset_name: String,
    pub corpus_root_name: String,
    pub embedding_dimension: usize,
    pub batch_size: usize,
    pub intra_threads: usize,
    pub candidate_oversample_factor: u32,
    pub hybrid_keyword_weight: f64,
    pub hybrid_vector_weight: f64,
    pub queries_sha256: String,
    pub qrels_sha256: String,
    pub corpus_manifest_sha256: String,
    pub positive_judgment_count: usize,
    pub explicit_negative_judgment_count: usize,
    pub judgment_sources: Vec<String>,
    pub operating_system: String,
    pub architecture: String,
    pub host_threads: usize,
    pub query_count: usize,
    pub answerable_query_count: usize,
    pub unanswerable_query_count: usize,
    pub corpus_file_count: usize,
    pub sensitive_files_skipped: usize,
    pub chunk_count: usize,
    pub tree_sitter_files_indexed: usize,
    pub existing_chunker_files_indexed: usize,
    pub tree_sitter_chunks_indexed: usize,
    pub existing_chunker_chunks_indexed: usize,
    pub repetitions: usize,
    pub model_load_ms: f64,
    pub indexing_ms: f64,
    pub database_bytes: u64,
    pub model_artifact_bytes: u64,
    pub embedding_ms: u64,
    pub embedding_calls: usize,
    pub embedding_chunks: usize,
    pub sqlite_transaction_ms: u64,
    pub warm_query_p50_ms: f64,
    pub warm_query_p95_ms: f64,
    pub overall: QualityMetrics,
    pub with_relation_neighbors: QualityMetrics,
    pub queries_with_relation_neighbors: usize,
    pub lexical_only: QualityMetrics,
    pub vector_only: QualityMetrics,
    pub unanswerable_hybrid: UnanswerableMetrics,
    pub unanswerable_lexical_only: UnanswerableMetrics,
    pub unanswerable_vector_only: UnanswerableMetrics,
    pub cases: Vec<EvalCaseReport>,
    pub by_category: BTreeMap<String, QualityMetrics>,
    pub by_language: BTreeMap<String, QualityMetrics>,
    pub citation_content_validity: Option<f64>,
    pub citation_content_matches: usize,
    pub citation_content_mismatches: Vec<CitationIssue>,
    pub citation_validity: f64,
    pub citation_count: usize,
    pub invalid_citations: Vec<CitationIssue>,
    pub filter_leaks: usize,
}

#[derive(Debug, Serialize)]
pub struct EvalCaseReport {
    pub query_id: String,
    pub category: String,
    pub language: String,
    pub answerability: QueryAnswerability,
    pub answerability_evidence: Option<String>,
    pub hybrid: Option<QualityMetrics>,
    pub lexical_only: Option<QualityMetrics>,
    pub vector_only: Option<QualityMetrics>,
    pub unanswerable_hybrid: Option<UnanswerableMetrics>,
    pub distinct_files_at_10: usize,
    pub top_paths: Vec<String>,
    pub lexical_top_paths: Vec<String>,
    pub vector_top_paths: Vec<String>,
    pub deep_paths: Vec<String>,
    pub layers: Vec<LayerReport>,
}

metric_record! {
    #[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
    pub struct UnanswerableMetrics {
        pub query_count: usize,
    }
    rates {
        pub explicit_negative_share_at_10: f64,
        pub judged_coverage_at_10: f64,
        pub empty_result_rate: f64,
    }
}

#[derive(Debug, Serialize)]
pub struct CitationIssue {
    pub query_id: String,
    pub path: String,
    pub lines: String,
}

#[cfg(test)]
mod tests;
