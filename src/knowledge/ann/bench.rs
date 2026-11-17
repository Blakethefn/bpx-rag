use std::path::PathBuf;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};

use super::build::build_index_scoped;
use super::sidecar::{load_sidecar_scoped, save_sidecar_scoped};
use super::update::{apply_incremental_changes, IncrementalChanges};

const MODEL: &str = "jinaai/jina-embeddings-v2-base-code";
const DIMENSIONS: usize = 768;
const SAFE_BENCH_ROOT: &str = "/media/blakethefn/2000HDD1/dev/project-assets/bpcontext/bench";

#[test]
#[ignore = "manual ANN HDD phase profile; set BPX_RAG_ANN_BENCH_DB to a replay copy"]
fn stale_sidecar_phases() {
    profile().unwrap();
}

fn profile() -> Result<()> {
    let path = std::env::var_os("BPX_RAG_ANN_BENCH_DB")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("BPX_RAG_ANN_BENCH_DB is required"))?;
    let path = path.canonicalize()?;
    let safe_root = PathBuf::from(SAFE_BENCH_ROOT).canonicalize()?;
    if !path.starts_with(&safe_root) {
        anyhow::bail!("ANN phase profiling is restricted to copies under {SAFE_BENCH_ROOT}");
    }
    let sidecar_dir = path
        .parent()
        .ok_or_else(|| anyhow!("benchmark database has no parent directory"))?
        .join("ann-indexes");
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open benchmark database {}", path.display()))?;
    let (generation, database_id): (i64, String) = conn.query_row(
        "SELECT generation, database_id FROM knowledge_embedding_state WHERE state_id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let expected_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_embeddings WHERE model_id = ?1",
        [MODEL],
        |row| row.get(0),
    )?;
    let expected_size = usize::try_from(expected_count)
        .context("embedding count does not fit the address space")?;
    let read_before = process_read_bytes()?;
    let started = Instant::now();
    let (index, manifest) = load_sidecar_scoped(
        &sidecar_dir,
        MODEL,
        DIMENSIONS,
        generation,
        &database_id,
        None,
    )?
    .ok_or_else(|| anyhow!("a compatible stale global ANN sidecar was not found"))?;
    let restore_ms = elapsed_ms(started);
    let restore_read_bytes = process_read_bytes()?.saturating_sub(read_before);
    anyhow::ensure!(
        manifest.generation < generation,
        "sidecar is already current"
    );

    let change_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_embedding_changes
         WHERE change_id > ?1 AND change_id <= ?2",
        [manifest.generation, generation],
        |row| row.get(0),
    )?;
    let changed_rows: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT chunk_rowid) FROM knowledge_embedding_changes
         WHERE change_id > ?1 AND change_id <= ?2",
        [manifest.generation, generation],
        |row| row.get(0),
    )?;
    let replay_read_before = process_read_bytes()?;
    let started = Instant::now();
    let tombstones = apply_incremental_changes(IncrementalChanges {
        conn: &conn,
        index: &index,
        model_id: MODEL,
        dimensions: DIMENSIONS,
        from_generation: manifest.generation,
        to_generation: generation,
        previous_tombstones: manifest.tombstones,
        expected_size,
        source_id: None,
    })?
    .ok_or_else(|| anyhow!("incremental replay fell back instead of completing"))?;
    let replay_ms = elapsed_ms(started);
    let replay_read_bytes = process_read_bytes()?.saturating_sub(replay_read_before);
    anyhow::ensure!(index.size() == expected_size, "replay size mismatch");

    let save_read_before = process_read_bytes()?;
    let started = Instant::now();
    save_sidecar_scoped(
        &sidecar_dir,
        &index,
        MODEL,
        DIMENSIONS,
        generation,
        &database_id,
        tombstones,
        None,
    )?;
    let save_ms = elapsed_ms(started);
    let save_read_bytes = process_read_bytes()?.saturating_sub(save_read_before);
    drop(index);

    let matched_read_before = process_read_bytes()?;
    let started = Instant::now();
    let (matched, matched_manifest) = load_sidecar_scoped(
        &sidecar_dir,
        MODEL,
        DIMENSIONS,
        generation,
        &database_id,
        None,
    )?
    .ok_or_else(|| anyhow!("saved matched sidecar could not be restored"))?;
    let matched_restore_ms = elapsed_ms(started);
    let matched_read_bytes = process_read_bytes()?.saturating_sub(matched_read_before);
    anyhow::ensure!(
        matched_manifest.generation == generation,
        "saved generation mismatch"
    );
    drop(matched);

    let rebuild_read_before = process_read_bytes()?;
    let started = Instant::now();
    let rebuilt = build_index_scoped(&conn, MODEL, DIMENSIONS, expected_size, None)?;
    let rebuild_ms = elapsed_ms(started);
    let rebuild_read_bytes = process_read_bytes()?.saturating_sub(rebuild_read_before);
    anyhow::ensure!(rebuilt.size() == expected_size, "rebuilt size mismatch");

    println!(
        "ANN phase profile: db_generation={generation} prior_generation={} \
         vectors={expected_size} changes={change_count} changed_rows={changed_rows} \
         restore_ms={restore_ms:.3} restore_read_bytes={restore_read_bytes} \
         replay_ms={replay_ms:.3} replay_read_bytes={replay_read_bytes} \
         sidecar_save_ms={save_ms:.3} sidecar_save_read_bytes={save_read_bytes} \
         matched_restore_ms={matched_restore_ms:.3} matched_restore_read_bytes={matched_read_bytes} \
         full_rebuild_ms={rebuild_ms:.3} full_rebuild_read_bytes={rebuild_read_bytes} \
         tombstones={tombstones}",
        manifest.generation
    );
    Ok(())
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

fn process_read_bytes() -> Result<u64> {
    let contents = std::fs::read_to_string("/proc/self/io")?;
    let value = contents
        .lines()
        .find_map(|line| line.strip_prefix("read_bytes: "))
        .ok_or_else(|| anyhow!("process read_bytes counter is unavailable"))?;
    Ok(value.parse()?)
}
