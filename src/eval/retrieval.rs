use super::*;
use crate::config::Config;
use crate::embed::{self, Embed, Embedder};
use crate::knowledge;
use crate::knowledge::query::SearchWeights;
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

mod dataset;
mod metrics;
mod report;
pub(super) use dataset::*;
pub(super) use metrics::*;
pub use report::*;

pub fn run_retrieval_evaluation(
    repetitions: usize,
    dataset: Option<&Path>,
    corpus_root: Option<&Path>,
    hybrid_weights: SearchWeights,
) -> Result<RetrievalEvalReport> {
    if !(1..=100).contains(&repetitions) {
        return Err(anyhow!("repetitions must be between 1 and 100"));
    }
    if !hybrid_weights.keyword_weight.is_finite()
        || !hybrid_weights.vector_weight.is_finite()
        || !(0.0..=10.0).contains(&hybrid_weights.keyword_weight)
        || !(0.0..=10.0).contains(&hybrid_weights.vector_weight)
        || hybrid_weights.keyword_weight == 0.0
        || hybrid_weights.vector_weight == 0.0
    {
        anyhow::bail!("hybrid weights must be finite and in the range (0, 10]");
    }
    let config = Config::load()?;
    let model_dir = crate::app::model_directory(&config);
    if !embed::model_files_present(&model_dir, &config.embeddings.model) {
        return Err(anyhow!(
            "evaluation requires the configured model to be cached; run `bpx-rag models download` first ({}).",
            model_dir.display()
        ));
    }

    let model_start = Instant::now();
    let embedder = Embedder::load_cached_with_device(
        &model_dir,
        &config.embeddings.model,
        config.embeddings.batch_size,
        config.embeddings.intra_threads,
        config.embeddings.device,
    )?;
    let model_load_ms = model_start.elapsed().as_secs_f64() * 1000.0;
    let model_id = embedder.model_id().to_string();
    let embedding_dimension = embedder.dim();
    let model_artifact_bytes = embed::model_cache_bytes(&model_dir, &config.embeddings.model)?;

    let data_dir = dataset.map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/retrieval"),
        Path::to_path_buf,
    );
    let corpus_dir = corpus_root.map_or_else(|| data_dir.join("corpus"), Path::to_path_buf);
    if !data_dir.is_dir() {
        anyhow::bail!(
            "retrieval dataset directory does not exist: {}",
            data_dir.display()
        );
    }
    if !corpus_dir.is_dir() {
        anyhow::bail!(
            "retrieval corpus directory does not exist: {}",
            corpus_dir.display()
        );
    }
    let query_path = data_dir.join("queries.jsonl");
    let qrels_path = data_dir.join("qrels.jsonl");
    let queries = read_jsonl::<EvalQuery>(&query_path)?;
    let qrels = read_qrels(&qrels_path)?;
    validate_qrels(&queries, &qrels, &corpus_dir)?;
    if queries.is_empty() || qrels.is_empty() {
        return Err(anyhow!("retrieval evaluation data is empty"));
    }
    let queries_sha256 = sha256_file(&query_path)?;
    let qrels_sha256 = sha256_file(&qrels_path)?;
    let corpus_manifest_hash = corpus_manifest_sha256(&corpus_dir)?;
    let positive_judgment_count = qrels
        .values()
        .flat_map(|judgments| judgments.values())
        .filter(|grade| **grade > 0)
        .count();
    let explicit_negative_judgment_count = qrels
        .values()
        .flat_map(|judgments| judgments.values())
        .filter(|grade| **grade == 0)
        .count();
    let mut judgment_sources = queries
        .iter()
        .filter_map(|query| query.judgment_source.as_deref())
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if judgment_sources.is_empty() {
        judgment_sources.push("unspecified".to_string());
    }

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

    let mut overall = MetricSums::default();
    let mut with_relations = MetricSums::default();
    let mut queries_with_neighbors = 0usize;
    let mut lexical_only = MetricSums::default();
    let mut vector_only = MetricSums::default();
    let mut categories: BTreeMap<String, MetricSums> = BTreeMap::new();
    let mut languages: BTreeMap<String, MetricSums> = BTreeMap::new();
    let mut unanswerable_hybrid = UnanswerableMetricSums::default();
    let mut unanswerable_lexical = UnanswerableMetricSums::default();
    let mut unanswerable_vector = UnanswerableMetricSums::default();
    let mut cases = Vec::with_capacity(queries.len());
    let mut latencies = Vec::with_capacity(queries.len() * repetitions);
    let mut citations_checked = 0usize;
    let mut valid_citations = 0usize;
    let mut invalid_citations = Vec::new();
    let mut citation_content_matches = 0usize;
    let mut citation_content_mismatches = Vec::new();
    let mut filter_leaks = 0usize;

    for query in &queries {
        let judgments = qrels
            .get(&query.query_id)
            .ok_or_else(|| anyhow!("no relevance judgments for query {}", query.query_id))?;
        let _ = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &hybrid_weights,
            1024,
        )?;
        let lexical_results = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &SearchWeights {
                keyword_weight: 1.0,
                vector_weight: 0.0,
            },
            1024,
        )?;
        let vector_results = knowledge::query::knowledge_search(
            &store,
            &query.text,
            query.filter.as_deref(),
            Some("retrieval-eval"),
            TOP_K as u32,
            &SearchWeights {
                keyword_weight: 0.0,
                vector_weight: 1.0,
            },
            1024,
        )?;
        let mut results = Vec::new();
        let mut query_times = Vec::with_capacity(repetitions);
        for _ in 0..repetitions {
            let started = Instant::now();
            results = knowledge::query::knowledge_search(
                &store,
                &query.text,
                query.filter.as_deref(),
                Some("retrieval-eval"),
                TOP_K as u32,
                &hybrid_weights,
                1024,
            )?;
            query_times.push(started.elapsed());
        }
        latencies.extend(query_times.iter().copied());

        let ranked_paths = results
            .iter()
            .map(|result| result.file_path.clone())
            .collect::<Vec<_>>();
        let metrics = measure_query(&ranked_paths, judgments);
        // The shipped rule: merge linked files of the top hit into the ranking.
        let expanded = crate::search::attach_relation_neighbors(
            &store,
            results.clone(),
            knowledge::links::RELATION_NEIGHBOR_LIMIT,
            TOP_K,
        )?;
        if expanded.len() != results.len()
            || expanded.iter().any(|result| result.why.graph_neighbor)
        {
            queries_with_neighbors += 1;
        }
        let expanded_paths = expanded
            .iter()
            .map(|result| result.file_path.clone())
            .collect::<Vec<_>>();
        with_relations.push(&measure_query(&expanded_paths, judgments));
        let lexical_metrics = measure_query(
            &lexical_results
                .iter()
                .map(|result| result.file_path.clone())
                .collect::<Vec<_>>(),
            judgments,
        );
        let vector_metrics = measure_query(
            &vector_results
                .iter()
                .map(|result| result.file_path.clone())
                .collect::<Vec<_>>(),
            judgments,
        );
        let no_answer_metrics = if query.answerability == QueryAnswerability::UnanswerableInCorpus {
            unanswerable_hybrid.push(&ranked_paths, judgments);
            unanswerable_lexical.push(
                &lexical_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                judgments,
            );
            unanswerable_vector.push(
                &vector_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                judgments,
            );
            Some(measure_unanswerable_query(&ranked_paths, judgments))
        } else {
            overall.push(&metrics);
            lexical_only.push(&lexical_metrics);
            vector_only.push(&vector_metrics);
            categories
                .entry(query.category.clone())
                .or_default()
                .push(&metrics);
            languages
                .entry(query.language.clone())
                .or_default()
                .push(&metrics);
            None
        };
        let is_answerable = query.answerability == QueryAnswerability::Answerable;
        cases.push(EvalCaseReport {
            query_id: query.query_id.clone(),
            category: query.category.clone(),
            language: query.language.clone(),
            answerability: query.answerability,
            answerability_evidence: query.answerability_evidence.clone(),
            hybrid: is_answerable.then_some(metrics.clone()),
            lexical_only: is_answerable.then_some(lexical_metrics),
            vector_only: is_answerable.then_some(vector_metrics),
            unanswerable_hybrid: no_answer_metrics,
            distinct_files_at_10: unique_paths(&ranked_paths, TOP_K).len(),
            top_paths: unique_paths(&ranked_paths, TOP_K),
            lexical_top_paths: unique_paths(
                &lexical_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            ),
            vector_top_paths: unique_paths(
                &vector_results
                    .iter()
                    .map(|result| result.file_path.clone())
                    .collect::<Vec<_>>(),
                TOP_K,
            ),
        });

        if let Some(expected_language) = &query.expected_language {
            for result in &results {
                let frontmatter_language = result.metadata["frontmatter"]["language"].as_str();
                if !crate::language::matches_language(
                    expected_language,
                    Path::new(&result.file_path),
                    frontmatter_language,
                ) {
                    filter_leaks += 1;
                }
            }
        }

        for result in &results {
            citations_checked += 1;
            let (citation_valid, content_matches) = citation_quality(
                &corpus_dir,
                &result.file_path,
                &result.lines,
                &result.snippet,
            )?;
            if citation_valid {
                valid_citations += 1;
            } else {
                invalid_citations.push(CitationIssue {
                    query_id: query.query_id.clone(),
                    path: result.file_path.clone(),
                    lines: result.lines.clone(),
                });
            }
            if content_matches {
                citation_content_matches += 1;
            } else {
                citation_content_mismatches.push(CitationIssue {
                    query_id: query.query_id.clone(),
                    path: result.file_path.clone(),
                    lines: result.lines.clone(),
                });
            }
        }
    }

    if sha256_file(&query_path)? != queries_sha256 || sha256_file(&qrels_path)? != qrels_sha256 {
        anyhow::bail!("retrieval evaluation dataset changed while it was running; retry");
    }
    if corpus_manifest_sha256(&corpus_dir)? != corpus_manifest_hash {
        anyhow::bail!("retrieval corpus changed while evaluation was running; retry");
    }

    latencies.sort_by(duration_cmp);
    let latency_ms = latencies
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .collect::<Vec<_>>();
    let db_bytes = in_memory_database_bytes(store.conn())?;

    Ok(RetrievalEvalReport {
        model_name: config.embeddings.model,
        model_id,
        embedding_device: config.embeddings.device.as_str().to_string(),
        dataset_name: data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("dataset")
            .to_string(),
        corpus_root_name: corpus_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("corpus")
            .to_string(),
        embedding_dimension,
        batch_size: config.embeddings.batch_size,
        intra_threads: config.embeddings.intra_threads,
        candidate_oversample_factor: knowledge::query::CANDIDATE_OVERSAMPLE_FACTOR,
        hybrid_keyword_weight: hybrid_weights.keyword_weight,
        hybrid_vector_weight: hybrid_weights.vector_weight,
        queries_sha256,
        qrels_sha256,
        corpus_manifest_sha256: corpus_manifest_hash,
        positive_judgment_count,
        explicit_negative_judgment_count,
        judgment_sources,
        operating_system: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        host_threads: std::thread::available_parallelism().map_or(1, usize::from),
        query_count: queries.len(),
        answerable_query_count: overall.count,
        unanswerable_query_count: unanswerable_hybrid.count,
        corpus_file_count: sync.files_scanned,
        sensitive_files_skipped: sync.sensitive_files_skipped,
        chunk_count: sync.chunks_total,
        tree_sitter_files_indexed: sync.tree_sitter_files_indexed,
        existing_chunker_files_indexed: sync.existing_chunker_files_indexed,
        tree_sitter_chunks_indexed: sync.tree_sitter_chunks_indexed,
        existing_chunker_chunks_indexed: sync.existing_chunker_chunks_indexed,
        repetitions,
        model_load_ms,
        indexing_ms,
        database_bytes: db_bytes,
        model_artifact_bytes,
        embedding_ms: sync.embedding_ms,
        embedding_calls: sync.embedding_calls,
        embedding_chunks: sync.embedding_chunks,
        sqlite_transaction_ms: sync.transaction_ms,
        warm_query_p50_ms: percentile(&latency_ms, 0.50),
        warm_query_p95_ms: percentile(&latency_ms, 0.95),
        overall: overall.average(),
        with_relation_neighbors: with_relations.average(),
        queries_with_relation_neighbors: queries_with_neighbors,
        lexical_only: lexical_only.average(),
        vector_only: vector_only.average(),
        unanswerable_hybrid: unanswerable_hybrid.average(),
        unanswerable_lexical_only: unanswerable_lexical.average(),
        unanswerable_vector_only: unanswerable_vector.average(),
        cases,
        by_category: categories
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        by_language: languages
            .into_iter()
            .map(|(key, sums)| (key, sums.average()))
            .collect(),
        citation_content_validity: (citations_checked > 0)
            .then_some(citation_content_matches as f64 / citations_checked as f64),
        citation_content_matches,
        citation_content_mismatches,
        citation_validity: if citations_checked == 0 {
            1.0
        } else {
            valid_citations as f64 / citations_checked as f64
        },
        citation_count: citations_checked,
        invalid_citations,
        filter_leaks,
    })
}
