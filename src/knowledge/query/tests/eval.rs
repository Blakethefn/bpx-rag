use super::super::*;
use super::{make_store, seed_chunk};
use crate::embed::Embed;
use rusqlite::params;
use std::sync::Arc;

struct EvaluationEmbedder;

impl EvaluationEmbedder {
    fn vector(text: &str) -> Vec<f32> {
        let text = text.to_lowercase();
        if text.contains("prevent users getting in")
            || text.contains("authorization")
            || text.contains("authentication")
            || text.contains("permission policy")
        {
            vec![0.0, 1.0, 0.0, 0.0]
        } else if text.contains("client lifetime")
            || text.contains("postgres connection pool")
            || text.contains("pool max lifetime")
        {
            vec![0.0, 0.0, 1.0, 0.0]
        } else if text.contains("queue acknowledgement")
            || text.contains("queue ack")
            || text.contains("durable queue")
        {
            vec![1.0, 0.0, 0.0, 0.0]
        } else {
            vec![0.0, 0.0, 0.0, 1.0]
        }
    }
}

impl Embed for EvaluationEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::vector(text))
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|text| Self::vector(text)).collect())
    }

    fn dim(&self) -> usize {
        4
    }
}

#[test]
fn deterministic_retrieval_eval_measures_recall_mrr_and_ndcg() {
    let mut store = make_store();
    store.set_embedder(Arc::new(EvaluationEmbedder));
    let corpus = [
        (
            "code",
            "src/queue/ack.rs",
            "Queue acknowledgement",
            "Durable queue acknowledgement retries until the message is committed.",
        ),
        (
            "code",
            "src/auth/policy.rs",
            "Permission policy",
            "Authorization checks verify roles before privileged actions are allowed.",
        ),
        (
            "library",
            "src/db/pool.rs",
            "Postgres connection pool",
            "The client lifetime config bounds pooled PostgreSQL connections.",
        ),
        (
            "guide",
            "docs/pool.md",
            "Pool max lifetime",
            "Postgres connection pool max lifetime controls how long idle clients remain reusable.",
        ),
        (
            "docs",
            "docs/retry.md",
            "Retry policy",
            "Backoff delays spread repeated network attempts across the queue.",
        ),
        (
            "docs",
            "docs/login.md",
            "Login session",
            "Sessions expire after a configured inactivity duration.",
        ),
        (
            "code",
            "src/cache/store.rs",
            "Cache eviction",
            "Least recently used entries are removed to keep memory bounded.",
        ),
        (
            "docs",
            "docs/monitoring.md",
            "Monitoring alerts",
            "Latency and error rate alerts notify the on-call engineer.",
        ),
    ];

    for (source, path, title, content) in corpus {
        let rowid = seed_chunk(&store, source, "/eval", path, title, content, "{}");
        let embedding = EvaluationEmbedder::vector(&format!("{title} {content}"));
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim) VALUES (?1, ?2, 4)",
                params![rowid, embed::embedding_to_bytes(&embedding)],
            )
            .unwrap();
    }

    let cases = [
        ("durable queue ack", None, "src/queue/ack.rs"),
        ("authentcation polciy", None, "src/auth/policy.rs"),
        ("prevent users getting in", None, "src/auth/policy.rs"),
        ("client lifetime", Some("library"), "src/db/pool.rs"),
    ];
    let query_count = cases.len() as f64;
    let mut reciprocal_rank_sum = 0.0;
    let mut ndcg_sum = 0.0;
    let mut recall_sum = 0.0;

    for (query, source, relevant_path) in cases {
        let results = knowledge_search(
            &store,
            query,
            None,
            source,
            10,
            &SearchWeights::default(),
            2000,
        )
        .unwrap();
        let ranks = results
            .iter()
            .map(|result| result.file_path.as_str())
            .collect::<Vec<_>>();
        let relevant_rank = ranks.iter().position(|path| *path == relevant_path);
        recall_sum += f64::from(relevant_rank.is_some());
        if let Some(rank) = relevant_rank {
            reciprocal_rank_sum += 1.0 / (rank as f64 + 1.0);
            ndcg_sum += 1.0 / ((rank as f64 + 2.0).log2());
        }
        assert!(
            results
                .iter()
                .all(|result| source.map_or(true, |source| result.source_label == source)),
            "source-filtered query returned an out-of-scope result"
        );
        assert!(
            relevant_rank.is_some(),
            "relevant result missing for query: {query}; got {ranks:?}"
        );
    }

    let recall_at_10 = recall_sum / query_count;
    let mrr_at_10 = reciprocal_rank_sum / query_count;
    let ndcg_at_10 = ndcg_sum / query_count;
    println!(
        "deterministic retrieval eval: Recall@10={recall_at_10:.3}, MRR@10={mrr_at_10:.3}, nDCG@10={ndcg_at_10:.3}"
    );
    assert!(recall_at_10 >= 1.0);
    assert!(mrr_at_10 >= 0.5);
    assert!(ndcg_at_10 >= 0.7);
}
