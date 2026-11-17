use anyhow::{ensure, Context, Result};
use rusqlite::{types::ToSql, Connection, OpenFlags};
use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const SAFE_ROOT: &str = "/media/blakethefn/2000HDD1/dev/project-assets/bpcontext/bench";
const POSIX_FADV_DONTNEED: i32 = 4;

unsafe extern "C" {
    fn posix_fadvise(fd: i32, offset: i64, len: i64, advice: i32) -> i32;
}

pub(super) fn benchmark_path(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("{name} is required"))?
        .canonicalize()?;
    let root = Path::new(SAFE_ROOT).canonicalize()?;
    ensure!(
        path.starts_with(&root),
        "benchmark database must stay under {SAFE_ROOT}"
    );
    Ok(path)
}

pub(super) fn verify_same_snapshot(baseline: &Path, candidate: &Path) -> Result<()> {
    let identity = |path: &Path| -> Result<(String, i64)> {
        Ok(open_readonly(path)?.query_row(
            "SELECT database_id,generation FROM knowledge_embedding_state WHERE state_id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    };
    ensure!(
        identity(baseline)? == identity(candidate)?,
        "benchmark copies differ in database identity or embedding generation"
    );
    Ok(())
}

pub(super) fn open_readonly(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}

pub(super) fn explain(
    conn: &Connection,
    sql: &str,
    params: &[Box<dyn ToSql>],
) -> Result<Vec<String>> {
    let refs = sql_params(params);
    Ok(conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
        .query_map(refs.as_slice(), |row| row.get(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(super) fn drop_db_cache(path: &Path) -> Result<()> {
    let file = File::open(path)?;
    let result = unsafe { posix_fadvise(file.as_raw_fd(), 0, 0, POSIX_FADV_DONTNEED) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(result).into())
    }
}

pub(super) fn process_read_bytes() -> Result<u64> {
    let contents = fs::read_to_string("/proc/self/io")?;
    let line = contents
        .lines()
        .find(|line| line.starts_with("read_bytes:"))
        .context("read_bytes missing")?;
    Ok(line
        .split_whitespace()
        .nth(1)
        .context("read_bytes value missing")?
        .parse()?)
}

fn sql_params(params: &[Box<dyn ToSql>]) -> Vec<&dyn ToSql> {
    params.iter().map(|param| param.as_ref()).collect()
}
