use anyhow::{ensure, Result};

use super::super::vector::knowledge_vector_search;
use crate::knowledge::filter::{file_scope_sql, scope_predicates, FilterPredicate};

mod io;
mod query;

pub(super) const MODEL: &str = "jinaai/jina-embeddings-v2-base-code";
const DEFAULT_SCOPE: &str = "dir:knowledge/query";
const LIMIT: u32 = 10;

#[test]
#[ignore = "one-pair HDD vector profile; set baseline/candidate DB env vars to isolated copies"]
fn file_first_vector_hdd_pair() {
    profile().unwrap();
}

struct VectorRun {
    elapsed_ms: f64,
    read_bytes: u64,
    vector_count: usize,
    rowids: Vec<i64>,
    score_bits: Vec<u32>,
}

fn profile() -> Result<()> {
    let baseline_path = io::benchmark_path("BPX_RAG_VECTOR_BENCH_BASELINE_DB")?;
    let candidate_path = io::benchmark_path("BPX_RAG_VECTOR_BENCH_CANDIDATE_DB")?;
    io::verify_same_snapshot(&baseline_path, &candidate_path)?;
    let scope =
        std::env::var("BPX_RAG_VECTOR_BENCH_FILTER").unwrap_or_else(|_| DEFAULT_SCOPE.to_string());
    let predicates = scope_predicates(Some(&scope), None)?;
    let path_predicates = path_predicates(&predicates);
    let (file_scope, file_params) = file_scope_sql(&path_predicates, 0)
        .ok_or_else(|| anyhow::anyhow!("directory predicate did not resolve"))?;
    let query_vector = query::deterministic_query_vector();
    let (legacy_sql, legacy_params) = query::legacy_sql(&predicates);
    let (candidate_sql, candidate_params) =
        query::file_first_sql(&file_scope, &file_params, &predicates)?;

    for (name, path, sql, params) in [
        (
            "baseline",
            baseline_path.as_path(),
            legacy_sql.as_str(),
            legacy_params.as_slice(),
        ),
        (
            "candidate",
            candidate_path.as_path(),
            candidate_sql.as_str(),
            candidate_params.as_slice(),
        ),
    ] {
        let conn = io::open_readonly(path)?;
        println!("{name}_plan={:?}", io::explain(&conn, sql, params)?);
    }

    let legacy = measure_legacy(&baseline_path, &legacy_sql, &legacy_params, &query_vector)?;
    let candidate = measure_candidate(
        &candidate_path,
        &predicates,
        &query_vector,
        legacy.vector_count,
    )?;
    ensure!(
        legacy.rowids == candidate.rowids,
        "baseline/candidate top-K IDs or order differ"
    );
    ensure!(
        legacy.score_bits == candidate.score_bits,
        "baseline/candidate exact dot-product scores differ"
    );
    println!("scope={scope} vectors={}", legacy.vector_count);
    println!(
        "baseline elapsed_ms={:.3} read_bytes={} top10_ids={:?} score_bits={:?}",
        legacy.elapsed_ms, legacy.read_bytes, legacy.rowids, legacy.score_bits
    );
    println!(
        "candidate elapsed_ms={:.3} read_bytes={} top10_ids={:?} score_bits={:?}",
        candidate.elapsed_ms, candidate.read_bytes, candidate.rowids, candidate.score_bits
    );
    Ok(())
}

fn path_predicates(predicates: &[FilterPredicate]) -> Vec<FilterPredicate> {
    predicates
        .iter()
        .filter(|predicate| {
            matches!(
                predicate,
                FilterPredicate::Directory(_) | FilterPredicate::File(_)
            )
        })
        .cloned()
        .collect()
}

fn measure_legacy(
    path: &std::path::Path,
    sql: &str,
    params: &[Box<dyn rusqlite::types::ToSql>],
    query: &[f32],
) -> Result<VectorRun> {
    io::drop_db_cache(path)?;
    let before = io::process_read_bytes()?;
    let started = std::time::Instant::now();
    let conn = io::open_readonly(path)?;
    let rows = query::legacy_rows(&conn, sql, params)?;
    let scored = query::score_rows(query, rows)?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let read_bytes = io::process_read_bytes()?.saturating_sub(before);
    Ok(VectorRun {
        elapsed_ms,
        read_bytes,
        vector_count: scored.len(),
        rowids: scored
            .iter()
            .take(LIMIT as usize)
            .map(|(rowid, _)| *rowid)
            .collect(),
        score_bits: scored
            .iter()
            .take(LIMIT as usize)
            .map(|(_, score)| score.to_bits())
            .collect(),
    })
}

fn measure_candidate(
    path: &std::path::Path,
    predicates: &[FilterPredicate],
    query: &[f32],
    vector_count: usize,
) -> Result<VectorRun> {
    io::drop_db_cache(path)?;
    let before = io::process_read_bytes()?;
    let started = std::time::Instant::now();
    let conn = io::open_readonly(path)?;
    let results = knowledge_vector_search(&conn, query, LIMIT, predicates, MODEL, None)?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let read_bytes = io::process_read_bytes()?.saturating_sub(before);
    let rowids = results
        .iter()
        .map(|result| result.rowid)
        .collect::<Vec<_>>();
    let scores = query::score_ids(&conn, query, &rowids)?;
    Ok(VectorRun {
        elapsed_ms,
        read_bytes,
        vector_count,
        rowids,
        score_bits: scores.iter().map(|score| score.to_bits()).collect(),
    })
}
