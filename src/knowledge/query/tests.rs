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

pub(super) fn make_store() -> KnowledgeStore {
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
pub(super) fn seed_chunk(
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

    // Insert matching identifier words, as indexing does
    conn.execute(
        "INSERT INTO knowledge_identifiers (rowid, words) VALUES (?1, ?2)",
        params![
            rowid,
            crate::knowledge::identifiers::identifier_words(&format!("{title} {content}"))
        ],
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

// ── Malformed database rows ──────────────────────────────────────────────
//
// A row that cannot be decoded is database corruption. Each retrieval layer
// must return an error for it, not drop the row or substitute a default.

/// Overwrite a chunk's metadata with bytes that cannot decode as TEXT.
fn corrupt_metadata_as_blob(store: &KnowledgeStore, chunk_rowid: i64) {
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET metadata = X'FFFE' WHERE chunk_rowid = ?1",
            params![chunk_rowid],
        )
        .unwrap();
}

#[test]
fn bm25_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn trigram_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_trigram_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn bm25_search_errors_on_non_integer_line_number_instead_of_zero() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lines.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET line_start = 'bad' WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "line numbers must not default to 0");
}

#[test]
fn vector_search_errors_on_non_integer_line_number_instead_of_zero() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lines.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, 2, 'test-model')",
            params![rowid, crate::embed::embedding_to_bytes(&[1.0, 0.0])],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET line_end = 'bad' WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();

    let result = knowledge_vector_search(store.conn(), &[1.0, 0.0], 5, "", &[], "test-model", None);

    assert!(result.is_err(), "line numbers must not default to 0");
}

#[test]
fn vector_search_rejects_embedding_with_wrong_byte_length() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/short.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    // Three bytes is not a whole f32 and does not match the two-dimension query.
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, 2, 'test-model')",
            params![rowid, vec![0u8, 0u8, 128u8]],
        )
        .unwrap();

    let result = knowledge_vector_search(store.conn(), &[1.0, 0.0], 5, "", &[], "test-model", None);

    assert!(
        result.is_err(),
        "a truncated embedding must not score silently"
    );
}

#[test]
fn search_errors_on_invalid_chunk_metadata_instead_of_defaulting_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/bad_meta.md",
        "Whale notes",
        "whale migration notes",
        "{not json",
    );

    let result = knowledge_search_report(
        &store,
        "whale migration",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    );

    assert!(result.is_err(), "invalid metadata JSON must be reported");
}

#[test]
fn fuzzy_term_extraction_recreates_missing_vocab_table() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/vocab.md",
        "Whale notes",
        "whale migration routes",
        "{}",
    );
    store
        .conn()
        .execute_batch("DROP TABLE IF EXISTS knowledge_chunks_vocab")
        .unwrap();

    let terms = extract_knowledge_terms(store.conn()).expect("vocab table is recreated");

    assert!(terms.iter().any(|term| term == "whale"));
}

// ── fuzzy correction ─────────────────────────────────────────────────────

/// Filler word `index`: six consonants chosen from eight. Consonant-only words
/// have no Porter suffix or vowel to strip, so each one stays distinct after
/// stemming and the vocabulary count is exact. Every filler sorts after
/// "aardvark" and before "zookeep". The test misspellings share too few letters
/// with a filler to fall within the edit distance.
fn filler_word(index: usize) -> String {
    const FILLER_LETTERS: &[u8; 8] = b"bcdfghjk";
    (0..6)
        .rev()
        .map(|place| char::from(FILLER_LETTERS[(index >> (3 * place)) & 7]))
        .collect()
}

/// Insert filler vocabulary. The FTS rows have no chunk metadata, so they
/// add terms without adding search results.
fn seed_filler_vocabulary(store: &KnowledgeStore, filler_count: usize) {
    let fillers: Vec<String> = (0..filler_count).map(filler_word).collect();
    for batch in fillers.chunks(500) {
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_chunks (title, content) VALUES ('', ?1)",
                params![batch.join(" ")],
            )
            .unwrap();
    }
}

/// Number of distinct terms in the content vocabulary that fuzzy matching reads.
fn content_vocab_size(store: &KnowledgeStore) -> i64 {
    let conn = store.conn();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab \
         USING fts5vocab(knowledge_chunks, instance);",
    )
    .unwrap();
    conn.query_row(
        "SELECT COUNT(DISTINCT term) FROM knowledge_chunks_vocab WHERE col = 'content'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

/// Rowid and path of each result from the fuzzy layer alone, in result order. Empty when
/// the layer did not run.
fn fuzzy_hits(store: &KnowledgeStore, query: &str) -> Vec<(i64, String)> {
    knowledge_fuzzy_search(store, query, 10, "", &[])
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|result| (result.rowid, result.file_path))
        .collect()
}

/// Rowid and path of each result from BM25 on `query` as written.
fn bm25_hits(store: &KnowledgeStore, query: &str) -> Vec<(i64, String)> {
    knowledge_bm25_search(store.conn(), query, 10, "", &[])
        .unwrap()
        .into_iter()
        .map(|result| (result.rowid, result.file_path))
        .collect()
}

#[test]
fn fuzzy_corrects_misspelling_when_vocabulary_has_fewer_than_500_terms() {
    let store = make_store();
    let apple = seed_chunk(
        &store,
        "repo",
        "/repo",
        "fruit.md",
        "Fruit",
        "ripe apple",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "aple"),
        vec![(apple, "fruit.md".to_string())]
    );
}

#[test]
fn fuzzy_corrects_misspelling_of_term_that_sorts_first_in_vocabulary_above_500_terms() {
    let store = make_store();
    seed_filler_vocabulary(&store, 600);
    let aardvark = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "aardvark",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "aardvrk"),
        vec![(aardvark, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_corrects_misspelling_of_late_alphabet_term_in_vocabulary_above_500_terms() {
    let store = make_store();
    seed_filler_vocabulary(&store, 600);
    let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    assert_eq!(
        fuzzy_hits(&store, "zookeepr"),
        vec![(zoo, "zoo.md".to_string())]
    );
}

#[test]
fn fuzzy_correction_is_the_same_at_499_500_501_and_5000_terms() {
    for vocab_size in [499, 500, 501, 5000] {
        let store = make_store();
        seed_filler_vocabulary(&store, vocab_size - 1);
        let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

        assert_eq!(
            content_vocab_size(&store),
            vocab_size as i64,
            "vocabulary size for case {vocab_size}"
        );
        assert_eq!(
            fuzzy_hits(&store, "zookeepr"),
            vec![(zoo, "zoo.md".to_string())],
            "misspelling must be corrected at vocabulary size {vocab_size}"
        );
    }
}

#[test]
fn fuzzy_leaves_exact_match_words_unchanged() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "cat.md", "Cat", "cat", "{}");
    seed_chunk(&store, "repo", "/repo", "cot.md", "Cot", "cot", "{}");

    assert!(
        fuzzy_hits(&store, "cat").is_empty(),
        "an exact match must not be rewritten to a neighbour"
    );
    assert!(
        fuzzy_hits(&store, "Cat").is_empty(),
        "a case-only difference from an indexed term is an exact match"
    );
}

#[test]
fn fuzzy_leaves_words_without_a_candidate_within_distance_two_unchanged() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    assert!(fuzzy_hits(&store, "qwzxv").is_empty());
    // "zkeepr" is distance 3 from "zookeep", so it is not corrected.
    assert!(fuzzy_hits(&store, "zkeepr").is_empty());
    // Only the typo is corrected. The unknown word stays as written.
    assert_eq!(
        fuzzy_hits(&store, "zookeepr qwzxv"),
        bm25_hits(&store, "zookeep qwzxv")
    );
}

#[test]
fn fuzzy_corrects_within_distance_two() {
    let store = make_store();
    let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    // "zokeepr" needs one insertion and one deletion to reach "zookeep".
    assert_eq!(
        fuzzy_hits(&store, "zokeepr"),
        vec![(zoo, "zoo.md".to_string())]
    );
}

/// Wrap a connection the way `open_in_memory` does, for tests that need two
/// connections to one file.
fn store_over(conn: rusqlite::Connection) -> KnowledgeStore {
    KnowledgeStore {
        conn,
        embedder: None,
        vector_index_dir: None,
        vector_index: None,
        vector_index_state: None,
        fuzzy_cache: Default::default(),
        query_embedding_cache: Default::default(),
    }
}

#[test]
fn fuzzy_correction_sees_terms_added_after_an_earlier_search() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");
    assert!(fuzzy_hits(&store, "quokkaa").is_empty());

    let quokka = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_correction_forgets_terms_removed_after_an_earlier_search() {
    let store = make_store();
    let quokka = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );
    assert_eq!(
        fuzzy_hits(&store, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );

    store
        .conn()
        .execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![quokka],
        )
        .unwrap();

    assert!(fuzzy_hits(&store, "quokkaa").is_empty());
}

#[test]
fn fuzzy_correction_sees_terms_written_by_another_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    let reader = store_over(crate::db::open_db(&path).unwrap());
    crate::knowledge::schema::apply_schema(reader.conn()).unwrap();
    seed_chunk(&reader, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");
    assert!(fuzzy_hits(&reader, "quokkaa").is_empty());

    let writer = store_over(crate::db::open_db(&path).unwrap());
    let quokka = seed_chunk(
        &writer,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&reader, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_repeated_runs_return_identical_corrections_for_tied_candidates() {
    let store = make_store();
    let bag = seed_chunk(&store, "repo", "/repo", "bag.md", "Bag", "bag", "{}");
    seed_chunk(&store, "repo", "/repo", "bar.md", "Bar", "bar", "{}");

    // "bag" and "bar" are both distance 1 from "bat". The smaller term wins.
    let first = fuzzy_hits(&store, "bat");
    assert_eq!(first, vec![(bag, "bag.md".to_string())]);
    for _ in 0..20 {
        assert_eq!(fuzzy_hits(&store, "bat"), first);
    }
}

#[test]
fn fuzzy_results_respect_source_and_metadata_filters() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "a.md",
        "A",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"task"}}"#,
    );
    seed_chunk(
        &store,
        "other",
        "/other",
        "b.md",
        "B",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"task"}}"#,
    );
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "c.md",
        "C",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"note"}}"#,
    );
    let weights = SearchWeights {
        keyword_weight: 1.0,
        vector_weight: 0.0,
    };
    let paths = |results: Vec<KnowledgeSearchResult>| -> Vec<String> {
        let mut paths: Vec<String> = results.into_iter().map(|r| r.file_path).collect();
        paths.sort();
        paths
    };

    let by_source =
        knowledge_search(&store, "zookeepr", None, Some("repo"), 10, &weights, 2000).unwrap();
    assert_eq!(paths(by_source), vec!["a.md", "c.md"]);

    let by_metadata = knowledge_search(
        &store,
        "zookeepr",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(paths(by_metadata), vec!["a.md", "b.md"]);

    let both = knowledge_search(
        &store,
        "zookeepr",
        Some("type:task"),
        Some("repo"),
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(paths(both), vec!["a.md"]);
}

// ── identifier layer ─────────────────────────────────────────────────────

/// Index `files` through the real sync path, so tests see the same rows that
/// indexing writes. The source is labelled `code`.
fn indexed_store(files: &[(&str, &str)]) -> (KnowledgeStore, tempfile::TempDir) {
    let store = make_store();
    let dir = tempfile::tempdir().unwrap();
    for (name, content) in files {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    store
        .add_source("code", &dir.path().to_string_lossy(), &[], &[])
        .unwrap();
    let source = store.get_source("code").unwrap().unwrap();
    crate::knowledge::sync::sync_source(&store, &source, None).unwrap();
    (store, dir)
}

fn file_paths(results: &[KnowledgeSearchResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect()
}

const GET_USER_BY_ID: &str = "export function getUserById(id) {\n  return users.get(id);\n}\n";

#[test]
fn identifier_layer_matches_a_natural_language_query_to_a_camel_case_identifier() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "get user by id", 10, "", &[]).unwrap();

    assert_eq!(file_paths_of_ranked(&hits), ["users.ts"]);
}

#[test]
fn search_finds_a_camel_case_identifier_from_split_words_when_no_other_layer_matches() {
    // Nothing in this chunk tokenizes to "get", "user", "by", or "id", so only
    // the identifier layer can match the query.
    let (store, _dir) = indexed_store(&[(
        "users.ts",
        "export function getUserById(lookup) { return cache.fetch(lookup); }\n",
    )]);

    let results = knowledge_search(
        &store,
        "get user by id",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert_eq!(file_paths(&results), ["users.ts"]);
}

#[test]
fn exact_identifier_query_still_returns_the_exact_chunk_first() {
    let (store, _dir) = indexed_store(&[
        ("users.ts", GET_USER_BY_ID),
        (
            "email.ts",
            "export function getUserByEmail(address) { return users.get(address); }\n",
        ),
    ]);

    let results = knowledge_search(
        &store,
        "getUserById",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert_eq!(results[0].file_path, "users.ts");
}

#[test]
fn indexed_content_keeps_the_original_identifier_and_stores_split_words_apart() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let content: String = store
        .conn()
        .query_row(
            "SELECT kc.content FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let words: String = store
        .conn()
        .query_row(
            "SELECT words FROM knowledge_identifiers LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        content.contains("export function getUserById(id) {\n  return users.get(id);\n}"),
        "content was rewritten: {content:?}"
    );
    assert_eq!(words, "get user by id");
}

#[test]
fn reindexing_a_file_removes_the_identifier_words_of_its_old_content() {
    let (store, dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);
    std::fs::write(
        dir.path().join("users.ts"),
        "export function listActiveUsers() { return []; }\n",
    )
    .unwrap();
    let source = store.get_source("code").unwrap().unwrap();
    crate::knowledge::sync::sync_source(&store, &source, None).unwrap();

    // Query terms are OR-combined, so a search cannot prove a row is gone. Check
    // the rows directly: only the new chunk's words may remain.
    let mut stmt = store
        .conn()
        .prepare("SELECT words FROM knowledge_identifiers")
        .unwrap();
    let words: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(words, ["list active users"]);

    let current =
        knowledge_identifier_search(store.conn(), "list active users", 10, "", &[]).unwrap();
    assert_eq!(file_paths_of_ranked(&current), ["users.ts"]);
}

#[test]
fn snake_and_kebab_query_terms_match_their_indexed_word_tokens() {
    // Characterization of D12: `sanitize_fts_terms` keeps `_` and `-`, and the
    // quoted phrase must match the tokens that unicode61 makes from them.
    let (store, _dir) = indexed_store(&[
        ("retry.py", "def get_user(identifier):\n    return None\n"),
        (
            "deploy.md",
            "Run the blue-green-deploy script before the release.\n",
        ),
    ]);

    let paths_for = |query: &str| {
        bm25_hits(&store, query)
            .into_iter()
            .map(|(_, path)| path)
            .collect::<Vec<_>>()
    };

    assert_eq!(paths_for("get_user"), ["retry.py"]);
    assert_eq!(paths_for("get-user"), ["retry.py"]);
    assert_eq!(paths_for("blue-green-deploy"), ["deploy.md"]);
}

fn file_paths_of_ranked(results: &[RankedResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect()
}

// ── identifier layer: selective matching ─────────────────────────────────

#[test]
fn identifier_layer_ignores_a_single_word_query() {
    // "user" is one content word, so no identifier match is attempted at all.
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "user", 10, "", &[]).unwrap();

    assert!(
        hits.is_empty(),
        "single-word query matched {:?}",
        file_paths_of_ranked(&hits)
    );
}

#[test]
fn identifier_layer_does_not_match_chunks_that_share_one_common_word() {
    // Both helpers contain the word "status" in their identifiers. Only the file with
    // all of "http", "status", and "code" may match.
    let (store, _dir) = indexed_store(&[
        ("http_server.rs", "pub fn HttpStatusCode() -> u16 { 200 }\n"),
        (
            "session_helpers.py",
            "def remove_session_status(identifier):\n    return None\n",
        ),
    ]);

    let hits = knowledge_identifier_search(store.conn(), "http status code", 10, "", &[]).unwrap();

    assert_eq!(file_paths_of_ranked(&hits), ["http_server.rs"]);
}

#[test]
fn identifier_layer_needs_two_content_words_not_stopwords() {
    // "by" and "the" are stopwords, so "by the id" has one content word.
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "by the id", 10, "", &[]).unwrap();

    assert!(
        hits.is_empty(),
        "one content word matched {:?}",
        file_paths_of_ranked(&hits)
    );
}

#[test]
fn identifier_layer_respects_source_label_filter() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);
    let results = knowledge_search(
        &store,
        "get user by id",
        None,
        Some("other-source"),
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert!(results.is_empty());
}

#[test]
fn identifier_layer_results_are_deterministic_across_repeated_searches() {
    let (store, _dir) = indexed_store(&[
        ("users.ts", GET_USER_BY_ID),
        (
            "accounts.ts",
            "export function getAccountById(id) { return id; }\n",
        ),
    ]);
    let run = || {
        knowledge_search(
            &store,
            "get user by id",
            None,
            None,
            10,
            &SearchWeights::default(),
            2000,
        )
        .unwrap()
        .into_iter()
        .map(|result| (result.file_path, result.score.to_bits()))
        .collect::<Vec<_>>()
    };

    assert_eq!(run(), run());
}
