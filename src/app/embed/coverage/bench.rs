use super::*;
use crate::app::SearchOptions;
use crate::config::Config;
use crate::embed::{Embed, tests::MockEmbedder};
use crate::knowledge::filter::scope_predicates;
use crate::knowledge::open_in_memory;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn seed(size: usize) -> RagApp {
    let mut knowledge = open_in_memory().unwrap();
    knowledge.set_embedder(Arc::new(MockEmbedder::new()));
    let source_id = knowledge
        .add_source("coverage-bench", "/synthetic/coverage", &[], &[])
        .unwrap();
    let conn = knowledge.conn();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    for file_index in 0..size / 10 {
        let directory = if file_index < size / 100 {
            "target"
        } else {
            "other"
        };
        let path = format!("src/{directory}/file-{file_index}.rs");
        conn.execute(
            "INSERT INTO knowledge_files
             (source_id, rel_path, content_hash, size_bytes, chunk_count)
             VALUES (?1, ?2, 'hash', 100, 10)",
            rusqlite::params![source_id, path],
        )
        .unwrap();
        let file_id = conn.last_insert_rowid();
        for chunk_index in 0..10 {
            conn.execute(
                "INSERT INTO knowledge_chunks (title, content)
                 VALUES ('synthetic', 'synthetic code content')",
                [],
            )
            .unwrap();
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO knowledge_chunk_meta
                 (chunk_rowid, file_id, source_id, metadata)
                 VALUES (?1, ?2, ?3, '{\"frontmatter\":{\"type\":\"code\"}}')",
                rusqlite::params![rowid, file_id, source_id],
            )
            .unwrap();
            if chunk_index % 5 != 0 {
                let vector = vec![0u8; MockEmbedder::new().dim() * 4];
                conn.execute(
                    "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        rowid,
                        vector,
                        MockEmbedder::new().dim() as i64,
                        Embed::model_id(&MockEmbedder::new())
                    ],
                )
                .unwrap();
            }
        }
    }
    conn.execute_batch("COMMIT").unwrap();
    let mut config = Config::default();
    config.search.keyword_weight = 0.0;
    config.search.vector_weight = 1.0;
    RagApp {
        config,
        knowledge,
        embedding_notice: None,
        embedder_loaded: true,
    }
}

fn measure(app: &RagApp, filter: &str, repetitions: usize) -> (i64, i64, f64, f64) {
    let predicates = scope_predicates(Some(filter), None).unwrap();
    let mut samples = Vec::with_capacity(repetitions);
    let mut coverage = None;
    for _ in 0..repetitions {
        let started = Instant::now();
        coverage = app.embedding_coverage(&predicates).unwrap();
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let coverage = coverage.unwrap();
    (
        coverage.total,
        coverage.current_model,
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
    )
}

fn measure_search(app: &mut RagApp, filter: &str, repetitions: usize) -> (f64, f64, f64, usize) {
    let options = SearchOptions {
        filter: Some(filter),
        ..SearchOptions::default()
    };
    let started = Instant::now();
    app.search("synthetic code content", options.clone())
        .unwrap();
    let cold_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let mut samples = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let started = Instant::now();
        app.search("synthetic code content", options.clone())
            .unwrap();
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p50_ms = percentile(&samples, 0.50);
    let p95_ms = percentile(&samples, 0.95);
    let index_bytes = app.knowledge.vector_index_memory_usage();
    (cold_ms, p50_ms, p95_ms, index_bytes)
}

fn percentile(samples: &[Duration], ratio: f64) -> f64 {
    let index = ((samples.len() - 1) as f64 * ratio).round() as usize;
    samples[index].as_secs_f64() * 1_000.0
}

#[test]
#[ignore = "manual scoped coverage timing benchmark"]
fn scoped_coverage_timing() {
    for size in [1_000, 10_000, 50_000] {
        let mut app = seed(size);
        for filter in [
            "source:coverage-bench",
            "source:coverage-bench dir:src/target",
            "source:coverage-bench file:src/target/file-0.rs",
            "source:coverage-bench type:code",
        ] {
            let (total, embedded, p50_ms, p95_ms) = measure(&app, filter, 20);
            println!(
                "chunks={size} scope={filter:?} total={total} embedded={embedded} p50_ms={p50_ms:.3} p95_ms={p95_ms:.3}"
            );
        }
        let search_scopes = if size < 16_384 {
            vec![
                "source:coverage-bench",
                "source:coverage-bench dir:src/target",
                "source:coverage-bench file:src/target/file-0.rs",
            ]
        } else {
            vec![
                "source:coverage-bench dir:src/target",
                "source:coverage-bench file:src/target/file-0.rs",
            ]
        };
        for filter in search_scopes {
            let (cold_ms, p50_ms, p95_ms, index_bytes) = measure_search(&mut app, filter, 3);
            println!(
                "search chunks={size} scope={filter:?} cold_ms={cold_ms:.3} warm_p50_ms={p50_ms:.3} warm_p95_ms={p95_ms:.3} ann_bytes={index_bytes}"
            );
        }
    }
}
