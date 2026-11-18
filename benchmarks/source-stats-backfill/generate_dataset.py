"""Generate a synthetic bpx-rag knowledge database of a chosen size.

The tables and indexes match bpx-rag's schema for the columns the source_stats
backfill reads. Embedding bytes are random, drawn from a fixed pool of blobs at
768 float32 dimensions. Coverage rows are computed from the embedding rows at
the end, which is the state the coverage triggers produce for a fully embedded
database with one model.

Usage:
  python3 generate_dataset.py --sources 50 --files-per-source 2 --chunks-per-file 10 --out small.db
  python3 generate_dataset.py --sources 50 --files-per-source 200 --chunks-per-file 100 --out large.db
"""
import argparse
import os
import random
import sqlite3
from pathlib import Path

MODEL_ID = "jinaai/jina-embeddings-v2-base-code"
DIM = 768
BLOB_POOL = 256
BATCH = 50_000

SCHEMA = """
CREATE TABLE knowledge_sources (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    label       TEXT NOT NULL UNIQUE,
    path        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    file_count  INTEGER NOT NULL DEFAULT 0,
    chunk_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE knowledge_files (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id    INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    rel_path     TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    indexed_at   TEXT NOT NULL DEFAULT (datetime('now')),
    chunk_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE(source_id, rel_path)
);
CREATE TABLE knowledge_chunk_meta (
    chunk_rowid INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
    source_id   INTEGER NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    line_start  INTEGER,
    line_end    INTEGER,
    metadata    TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE knowledge_embeddings (
    chunk_rowid INTEGER PRIMARY KEY,
    embedding   BLOB NOT NULL,
    dim         INTEGER NOT NULL DEFAULT 384,
    model_id    TEXT NOT NULL DEFAULT 'sentence-transformers/all-MiniLM-L6-v2'
);
CREATE TABLE knowledge_embedding_coverage (
    file_id        INTEGER NOT NULL REFERENCES knowledge_files(id) ON DELETE CASCADE,
    model_id       TEXT NOT NULL,
    embedded_count INTEGER NOT NULL CHECK (embedded_count >= 0),
    PRIMARY KEY (file_id, model_id)
);
CREATE TABLE knowledge_index_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE INDEX idx_files_source ON knowledge_files(source_id);
CREATE INDEX idx_chunk_meta_source ON knowledge_chunk_meta(source_id);
CREATE INDEX idx_chunk_meta_file ON knowledge_chunk_meta(file_id);
CREATE INDEX idx_embeddings_model ON knowledge_embeddings(model_id);
CREATE INDEX idx_embedding_coverage_model_file ON knowledge_embedding_coverage(model_id, file_id);
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--sources", type=int, required=True)
    parser.add_argument("--files-per-source", type=int, required=True)
    parser.add_argument("--chunks-per-file", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    if args.out.exists():
        raise SystemExit(f"refusing to overwrite {args.out}")
    rng = random.Random(args.seed)
    blobs = [os.urandom(DIM * 4) for _ in range(BLOB_POOL)]
    conn = sqlite3.connect(args.out, isolation_level=None)
    conn.executescript(SCHEMA)
    conn.execute("BEGIN")
    for s in range(1, args.sources + 1):
        conn.execute("INSERT INTO knowledge_sources(id, label, path) VALUES (?, ?, ?)",
                     (s, f"source-{s:03d}", f"synthetic/source-{s:03d}"))
    file_id = 0
    chunk_id = 0
    pending = []
    for s in range(1, args.sources + 1):
        for f in range(args.files_per_source):
            file_id += 1
            conn.execute(
                "INSERT INTO knowledge_files(id, source_id, rel_path, content_hash, size_bytes, chunk_count) "
                "VALUES (?, ?, ?, ?, ?, ?)",
                (file_id, s, f"file-{f:04d}.txt", f"{rng.getrandbits(128):032x}",
                 args.chunks_per_file * 512, args.chunks_per_file))
            for c in range(args.chunks_per_file):
                chunk_id += 1
                pending.append((chunk_id, file_id, s, c * 10 + 1, c * 10 + 10, "{}", blobs[chunk_id % BLOB_POOL]))
                if len(pending) >= BATCH:
                    flush(conn, pending)
                    pending = []
    flush(conn, pending)
    conn.execute("COMMIT")
    conn.execute("BEGIN")
    conn.execute(
        "INSERT INTO knowledge_embedding_coverage(file_id, model_id, embedded_count) "
        "SELECT m.file_id, ?, COUNT(*) FROM knowledge_chunk_meta m GROUP BY m.file_id",
        (MODEL_ID,))
    conn.execute("COMMIT")
    conn.execute("ANALYZE")
    counts = conn.execute(
        "SELECT (SELECT COUNT(*) FROM knowledge_sources), (SELECT COUNT(*) FROM knowledge_files), "
        "(SELECT COUNT(*) FROM knowledge_chunk_meta), (SELECT COUNT(*) FROM knowledge_embeddings)").fetchone()
    conn.close()
    print(f"wrote {args.out.name}: sources={counts[0]} files={counts[1]} chunks={counts[2]} embeddings={counts[3]}")
    return 0


def flush(conn: sqlite3.Connection, rows) -> None:
    if not rows:
        return
    conn.executemany(
        "INSERT INTO knowledge_chunk_meta(chunk_rowid, file_id, source_id, line_start, line_end, metadata) "
        "VALUES (?, ?, ?, ?, ?, ?)",
        [(r[0], r[1], r[2], r[3], r[4], r[5]) for r in rows])
    conn.executemany(
        "INSERT INTO knowledge_embeddings(chunk_rowid, embedding, dim, model_id) VALUES (?, ?, ?, ?)",
        [(r[0], r[6], DIM, MODEL_ID) for r in rows])


if __name__ == "__main__":
    raise SystemExit(main())
