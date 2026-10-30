use super::*;
use crate::embed::Embed;
use crate::knowledge;
use rusqlite::params;
use std::sync::Arc;

struct SemanticTestEmbedder;

impl Embed for SemanticTestEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(
            if text == "whale" || text.contains("blue ocean migration") {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            },
        )
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }

    fn dim(&self) -> usize {
        2
    }
}

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

fn make_store() -> KnowledgeStore {
    knowledge::open_in_memory().expect("in-memory knowledge store")
}

#[test]
fn lexical_search_matches_individual_terms_in_natural_language_queries() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/search.rs",
        "Hybrid retrieval layers",
        "The pipeline runs BM25, trigram, fuzzy, and semantic vector retrieval, then merges ranked results.",
        "{}",
    );

    let results = knowledge_search(
        &store,
        "Which code runs BM25 and semantic vector retrieval across the corpus?",
        None,
        Some("repo"),
        10,
        &SearchWeights {
            keyword_weight: 1.0,
            vector_weight: 0.0,
        },
        2000,
    )
    .unwrap();

    assert_eq!(results[0].file_path, "src/search.rs");
    assert!(results[0].why.layers.bm25 > 0.0);
}

#[test]
fn dense_retrieval_can_find_a_chunk_with_no_lexical_hit() {
    let mut store = make_store();
    store.set_embedder(Arc::new(SemanticTestEmbedder));
    let lexical = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lexical.md",
        "Lexical distractor",
        "whale appears in a glossary but does not describe movement",
        "{}",
    );
    let semantic = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/migration.md",
        "Ocean migration",
        "blue ocean migration paths of large mammals",
        "{}",
    );
    for (rowid, content) in [
        (
            lexical,
            "whale appears in a glossary but does not describe movement",
        ),
        (semantic, "blue ocean migration paths of large mammals"),
    ] {
        let embedding = SemanticTestEmbedder.embed_one(content).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim) VALUES (?1, ?2, 2)",
                params![rowid, embed::embedding_to_bytes(&embedding)],
            )
            .unwrap();
    }

    let results = knowledge_search(
        &store,
        "whale",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert!(results.iter().any(|result| {
        result.file_path == "src/migration.md" && result.why.layers.vector > 0.0
    }));
}

#[test]
fn dense_retrieval_ignores_vectors_from_a_different_model() {
    let mut store = make_store();
    store.set_embedder(Arc::new(SemanticTestEmbedder));
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/migration.md",
        "Ocean migration",
        "blue ocean migration paths of large mammals",
        "{}",
    );
    let incompatible = SemanticTestEmbedder
        .embed_one("blue ocean migration")
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, ?2, 2, 'jinaai/jina-embeddings-v2-base-code')",
            params![rowid, embed::embedding_to_bytes(&incompatible)],
        )
        .unwrap();

    let results = knowledge_search(
        &store,
        "whale",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert!(
        results.is_empty(),
        "vectors from a foreign model must not be compared"
    );
}

#[test]
fn explicit_empty_vector_candidates_return_no_results() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/vector.md",
        "Vector chunk",
        "text that would otherwise be searchable",
        "{}",
    );
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim)
             VALUES (?1, ?2, 2)",
            params![rowid, embed::embedding_to_bytes(&[1.0, 0.0])],
        )
        .unwrap();

    let results = knowledge_vector_search(
        store.conn(),
        &[1.0, 0.0],
        10,
        "",
        &[],
        "sentence-transformers/all-MiniLM-L6-v2",
        Some(&[]),
    )
    .unwrap();

    assert!(results.is_empty());
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

/// Insert a source + file + chunk with metadata for testing search.
fn seed_chunk(
    store: &KnowledgeStore,
    source_label: &str,
    source_path: &str,
    rel_path: &str,
    title: &str,
    content: &str,
    metadata_json: &str,
) -> i64 {
    let conn = store.conn();

    // Ensure source exists
    let source_id: i64 = match conn.query_row(
        "SELECT id FROM knowledge_sources WHERE label = ?1",
        params![source_label],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO knowledge_sources (label, path, enrichments) VALUES (?1, ?2, '[]')",
                params![source_label, source_path],
            )
            .unwrap();
            conn.last_insert_rowid()
        }
    };

    // Ensure file exists
    let file_id: i64 = match conn.query_row(
        "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = ?2",
        params![source_id, rel_path],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes) VALUES (?1, ?2, 'abc123', 100)",
                params![source_id, rel_path],
            )
            .unwrap();
            conn.last_insert_rowid()
        }
    };

    // Insert into FTS5
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
        params![title, content],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();

    // Insert matching trigram row
    conn.execute(
        "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
        params![rowid, title, content],
    )
    .unwrap();

    // Insert chunk_meta
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata) VALUES (?1, ?2, ?3, 1, 20, ?4)",
        params![rowid, file_id, source_id, metadata_json],
    )
    .unwrap();

    rowid
}

// ── sanitize helpers ─────────────────────────────────────────────────────

#[test]
fn test_sanitize_fts_query() {
    assert_eq!(
        sanitize_fts_query("hello world"),
        "\"hello world\" OR \"hello\" OR \"world\""
    );
    assert_eq!(
        sanitize_fts_query("error: can't find"),
        "\"error can t find\" OR \"error\" OR \"can\" OR \"t\" OR \"find\""
    );
    assert_eq!(
        sanitize_fts_query("fn main()"),
        "\"fn main\" OR \"fn\" OR \"main\""
    );
}

#[test]
fn test_sanitize_trigram_query() {
    assert_eq!(sanitize_trigram_query("hello"), "\"hello\"");
    assert_eq!(sanitize_trigram_query(""), "");
}

#[test]
fn test_truncate_snippet_short() {
    let s = "short text";
    assert_eq!(truncate_snippet(s, 100), "short text");
}

#[test]
fn test_truncate_snippet_at_word_boundary() {
    let s = "the quick brown fox jumps over the lazy dog";
    let result = truncate_snippet(s, 20);
    assert!(result.ends_with("..."));
    assert!(result.len() <= 24); // 20 + "..."
}

#[test]
fn test_truncate_snippet_preserves_utf8_boundaries() {
    let result = truncate_snippet("before 🧑‍💻 after", 9);
    assert_eq!(result, "before...");
}

// ── knowledge_search (no filter) ─────────────────────────────────────────

#[test]
fn search_empty_db_returns_empty() {
    let store = make_store();
    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "anything", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty());
}

#[test]
fn search_finds_bm25_match() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "notes/test.md",
        "Fatigue Model",
        "The fatigue model applies minute-weighted decay to player performance",
        "{}",
    );

    let weights = SearchWeights::default();
    let results =
        knowledge_search(&store, "fatigue model", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].source_type, "knowledge");
    assert_eq!(results[0].source_label, "vault");
    assert_eq!(results[0].file_path, "notes/test.md");
}

#[test]
fn search_returns_correct_lines_format() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "test.md",
        "Title",
        "Content here",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "content", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].lines, "1-20");
}

// ── knowledge_search (with metadata filter) ──────────────────────────────

#[test]
fn search_with_frontmatter_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "task1.md",
        "Active Task",
        "This is an active task about implementation",
        r#"{"frontmatter":{"type":"task","status":"active"}}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "note1.md",
        "Random Note",
        "This is a random note about implementation",
        r#"{"frontmatter":{"type":"note","status":"draft"}}"#,
    );

    let weights = SearchWeights::default();

    // Filter for type:task — should return only the task
    let results = knowledge_search(
        &store,
        "implementation",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Active Task");
}

#[test]
fn search_with_source_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "v.md",
        "Vault Note",
        "Content from vault about algorithms",
        "{}",
    );
    seed_chunk(
        &store,
        "docs",
        "/tmp/docs",
        "d.md",
        "Docs Note",
        "Content from docs about algorithms",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "algorithms",
        Some("source:vault"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "vault");
}

#[test]
fn search_with_tag_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "tagged.md",
        "Tagged Note",
        "Content about prediction models and forecasting",
        r#"{"frontmatter":{"tags":["implementation","system3"]}}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "untagged.md",
        "Untagged Note",
        "Content about prediction models and estimation",
        r#"{"frontmatter":{"tags":["review"]}}"#,
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "prediction",
        Some("tag:implementation"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Tagged Note");
}

#[test]
fn search_with_folder_filter() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "projects/foo.md",
        "Project Note",
        "Content about project setup and configuration",
        r#"{"folder_tags":["01-projects","foo"]}"#,
    );
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "logs/bar.md",
        "Log Note",
        "Content about debugging and configuration",
        r#"{"folder_tags":["06-logs","debug"]}"#,
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "configuration",
        Some("folder:01-projects"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Project Note");
}

#[test]
fn search_with_source_label_param() {
    let store = make_store();
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "v.md",
        "Vault Item",
        "Searchable content about architecture",
        "{}",
    );
    seed_chunk(
        &store,
        "docs",
        "/tmp/docs",
        "d.md",
        "Docs Item",
        "Searchable content about architecture",
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(
        &store,
        "architecture",
        None,
        Some("docs"),
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "docs");
}

#[test]
fn search_metadata_in_result() {
    let store = make_store();
    let meta = r#"{"frontmatter":{"type":"task","status":"active"}}"#;
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "meta.md",
        "Meta Test",
        "Content with metadata for testing purposes",
        meta,
    );

    let weights = SearchWeights::default();
    let results =
        knowledge_search(&store, "metadata testing", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty());
    let fm = &results[0].metadata["frontmatter"];
    assert_eq!(fm["type"], "task");
    assert_eq!(fm["status"], "active");
}

// ── combined_search ──────────────────────────────────────────────────────

#[test]
fn search_snippet_truncated() {
    let store = make_store();
    let long_content = "word ".repeat(500); // ~2500 chars
    seed_chunk(
        &store,
        "vault",
        "/tmp/vault",
        "long.md",
        "Long Content",
        &long_content,
        "{}",
    );

    let weights = SearchWeights::default();
    let results = knowledge_search(&store, "word", None, None, 10, &weights, 100).unwrap();
    assert!(!results.is_empty());
    assert!(results[0].snippet.len() <= 104); // 100 + "..."
}

// ── dot_product ──────────────────────────────────────────────────────────

#[test]
fn test_dot_product_identical() {
    let a = vec![1.0, 0.0, 0.0];
    assert!((dot_product(&a, &a) - 1.0).abs() < 1e-6);
}

#[test]
fn test_dot_product_orthogonal() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![0.0, 1.0, 0.0];
    assert!(dot_product(&a, &b).abs() < 1e-6);
}

struct BrokenQueryEmbedder;

impl Embed for BrokenQueryEmbedder {
    fn embed_one(&self, _text: &str) -> Result<Vec<f32>> {
        anyhow::bail!("synthetic query embedding failure")
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed_one(text)).collect()
    }

    fn dim(&self) -> usize {
        4
    }

    fn max_batch_size(&self) -> usize {
        16
    }

    fn model_id(&self) -> &str {
        "broken-test-model"
    }
}

#[test]
fn failed_query_embedding_is_reported_and_lexical_results_still_return() {
    let mut store = make_store();
    store.set_embedder(Arc::new(BrokenQueryEmbedder));
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/whale.md",
        "Whale notes",
        "whale migration routes across the ocean",
        "{}",
    );

    let report = knowledge_search_report(
        &store,
        "whale migration",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    )
    .unwrap();

    assert!(!report.results.is_empty(), "lexical layers still answer");
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("vector search was skipped"));
    assert!(report.warnings[0].contains("synthetic query embedding failure"));
}
