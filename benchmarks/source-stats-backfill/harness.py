"""Like-for-like timing of the two source_stats backfill strategies.

Both strategies run in this one process, against the same working copy of the
database, under the same cache condition, alternating order between runs. Each
run is rolled back, so the working copy is never changed. The harness checks
that both strategies produce identical rows.

  before  correlated scalar subqueries per source (bpx-rag commit 812f5ed)
  after   LEFT JOIN to grouped aggregates, embedded counts summed from
          knowledge_embedding_coverage (bpx-rag commit 1f941f1 and later)

Usage:
  python3 harness.py --db PATH/knowledge.db --cache cold --runs 3 --out results-cold.json
  python3 harness.py --db PATH/knowledge.db --cache warm --runs 3 --out results-warm.json

The source database is opened read-only and copied with the SQLite backup API.
Use --workdir to choose where the working copy goes (default: the system temp
directory). Cold runs evict the working copy from the page cache before each run
with posix_fadvise(POSIX_FADV_DONTNEED), which needs Linux or another Unix that
provides it. Elsewhere the cold condition is recorded as not controlled.
"""
import argparse
import hashlib
import json
import os
import platform
import sqlite3
import statistics
import sys
import tempfile
import time
from pathlib import Path

BEFORE_SQL = """
DELETE FROM source_stats;
INSERT INTO source_stats(source_id, chunk_count, embedded_count, total_bytes, last_indexed_at)
SELECT s.id,
   (SELECT COUNT(*) FROM knowledge_chunk_meta m WHERE m.source_id = s.id),
   (SELECT COUNT(*) FROM knowledge_embeddings e
    JOIN knowledge_chunk_meta m ON m.chunk_rowid = e.chunk_rowid
    WHERE m.source_id = s.id),
   COALESCE((SELECT SUM(size_bytes) FROM knowledge_files f WHERE f.source_id = s.id), 0),
   (SELECT MAX(indexed_at) FROM knowledge_files f WHERE f.source_id = s.id)
FROM knowledge_sources s;
"""

AFTER_SQL = """
DELETE FROM source_stats;
INSERT INTO source_stats(source_id, chunk_count, embedded_count, total_bytes, last_indexed_at)
SELECT s.id,
   COALESCE(c.chunk_count, 0),
   COALESCE(e.embedded_count, 0),
   COALESCE(f.total_bytes, 0),
   f.last_indexed_at
FROM knowledge_sources s
LEFT JOIN (
   SELECT source_id, COUNT(*) AS chunk_count
   FROM knowledge_chunk_meta GROUP BY source_id
) c ON c.source_id = s.id
LEFT JOIN (
   SELECT f.source_id, SUM(cov.embedded_count) AS embedded_count
   FROM knowledge_embedding_coverage cov
   JOIN knowledge_files f ON f.id = cov.file_id
   GROUP BY f.source_id
) e ON e.source_id = s.id
LEFT JOIN (
   SELECT source_id, SUM(size_bytes) AS total_bytes, MAX(indexed_at) AS last_indexed_at
   FROM knowledge_files GROUP BY source_id
) f ON f.source_id = s.id;
"""

STATS_DDL = """
CREATE TABLE IF NOT EXISTS source_stats (
    source_id INTEGER PRIMARY KEY REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    chunk_count INTEGER NOT NULL DEFAULT 0 CHECK (chunk_count >= 0),
    embedded_count INTEGER NOT NULL DEFAULT 0 CHECK (embedded_count >= 0),
    total_bytes INTEGER NOT NULL DEFAULT 0 CHECK (total_bytes >= 0),
    last_indexed_at TEXT
);
"""

VARIANTS = {"before": BEFORE_SQL, "after": AFTER_SQL}


def evict(path: Path) -> bool:
    if not hasattr(os, "posix_fadvise"):
        return False
    fd = os.open(path, os.O_RDONLY)
    try:
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
    finally:
        os.close(fd)
    return True


def make_working_copy(source: Path, workdir: Path) -> Path:
    copy_path = workdir / "knowledge-working-copy.db"
    src = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    dst = sqlite3.connect(copy_path)
    try:
        src.backup(dst)
    finally:
        dst.close()
        src.close()
    conn = sqlite3.connect(copy_path)
    conn.executescript(STATS_DDL)
    conn.commit()
    conn.close()
    return copy_path


def digest(conn: sqlite3.Connection) -> str:
    rows = conn.execute(
        "SELECT source_id, chunk_count, embedded_count, total_bytes, IFNULL(last_indexed_at, '') "
        "FROM source_stats ORDER BY source_id"
    ).fetchall()
    return hashlib.sha256(repr(rows).encode()).hexdigest()


def timed_run(copy_path: Path, sql: str, cache: str, evictable: bool):
    if cache == "cold" and evictable:
        evict(copy_path)
    conn = sqlite3.connect(copy_path, isolation_level=None)
    try:
        conn.execute("BEGIN IMMEDIATE")
        statements = [part.strip() for part in sql.split(";") if part.strip()]
        start = time.perf_counter()
        for statement in statements:  # execute, not executescript: keep the open transaction
            conn.execute(statement)
        elapsed = time.perf_counter() - start
        rows_digest = digest(conn)
        conn.execute("ROLLBACK")
    finally:
        conn.close()
    return elapsed, rows_digest


def dataset_shape(source: Path) -> dict:
    conn = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    try:
        return {
            "sources": conn.execute("SELECT COUNT(*) FROM knowledge_sources").fetchone()[0],
            "files": conn.execute("SELECT COUNT(*) FROM knowledge_files").fetchone()[0],
            "chunks": conn.execute("SELECT COUNT(*) FROM knowledge_chunk_meta").fetchone()[0],
            "embedding_rows": conn.execute("SELECT COUNT(*) FROM knowledge_embeddings").fetchone()[0],
            "database_bytes": source.stat().st_size,
        }
    finally:
        conn.close()


def summarize(values):
    return {
        "seconds": [round(v, 6) for v in values],
        "median": round(statistics.median(values), 6),
        "min": round(min(values), 6),
        "max": round(max(values), 6),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--db", required=True, type=Path)
    parser.add_argument("--cache", choices=["cold", "warm"], required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--workdir", type=Path, default=None)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(dir=args.workdir) as tmp:
        workdir = Path(tmp)
        copy_path = make_working_copy(args.db, workdir)
        evictable = hasattr(os, "posix_fadvise")
        timings = {name: [] for name in VARIANTS}
        digests = {}

        if args.cache == "warm":
            for name, sql in VARIANTS.items():
                timed_run(copy_path, sql, "warm", evictable)  # warm-up, not recorded

        names = list(VARIANTS)
        for i in range(args.runs):
            order = names if i % 2 == 0 else list(reversed(names))  # alternate order
            for name in order:
                elapsed, rows_digest = timed_run(copy_path, VARIANTS[name], args.cache, evictable)
                timings[name].append(elapsed)
                digests.setdefault(name, rows_digest)
                print(f"{args.cache} {name} run {i + 1}: {elapsed:.3f} s", file=sys.stderr)

        outputs_match = len(set(digests.values())) == 1
        result = {
            "benchmark": "source-stats-backfill",
            "harness": "harness.py",
            "cache": args.cache,
            "cache_controlled": args.cache == "warm" or evictable,
            "runs_per_variant": args.runs,
            "dataset": dataset_shape(args.db),
            "environment": {
                "system": platform.system(),
                "machine": platform.machine(),
                "python": platform.python_version(),
                "sqlite": sqlite3.sqlite_version,
                "cpu_count": os.cpu_count(),
            },
            "variants": {name: summarize(vals) for name, vals in timings.items()},
            "ratio_of_medians": round(
                statistics.median(timings["before"]) / statistics.median(timings["after"]), 2
            ),
            "outputs_match": outputs_match,
        }
        args.out.write_text(json.dumps(result, indent=2) + "\n")
        print(f"wrote {args.out}", file=sys.stderr)
        if not outputs_match:
            print("ERROR: before and after produced different source_stats rows", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
