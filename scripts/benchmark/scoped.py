#!/usr/bin/env python3
"""Compare scoped FTS queries and Markdown coverage on two SQLite snapshots."""

import argparse
import hashlib
import json
import re
import sqlite3
import statistics
import time
from pathlib import Path

MODEL = "jinaai/jina-embeddings-v2-base-code"
STOPWORDS = {"a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "how", "if", "in", "into", "is", "it", "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where", "which", "with"}


def connect(path):
    connection = sqlite3.connect(f"file:{Path(path).resolve()}?mode=ro", uri=True)
    connection.execute("PRAGMA query_only = ON")
    return connection


def bm25_query(query):
    cleaned = re.sub(r"[^A-Za-z0-9_-]+", " ", query).strip()
    terms = cleaned.split()[:32]
    if not terms:
        return ""
    return " OR ".join([f'"{cleaned}"', *(f'"{term}"' for term in terms)])


def identifier_query(query):
    terms = [term.lower() for term in re.findall(r"[A-Za-z0-9_-]+", query)]
    terms = [term for term in terms if term not in STOPWORDS][:12]
    return " AND ".join(f'"{term}"' for term in terms) if len(terms) >= 2 else ""


def lexical_sql(layer, bounded):
    if layer == "bm25":
        table, from_clause = "knowledge_chunks", "knowledge_chunks kc"
        rowid = "kc.rowid"
    else:
        table = "knowledge_identifiers"
        from_clause = "knowledge_identifiers JOIN knowledge_chunks kc ON kc.rowid = knowledge_identifiers.rowid"
        rowid = "knowledge_identifiers.rowid"
    range_clause = f" AND {rowid} BETWEEN ? AND ?" if bounded else ""
    metadata_join = "CROSS JOIN" if bounded else "JOIN"
    return f"""SELECT {rowid}, kcm.file_id, kcm.source_id, kc.title, kc.content,
                     COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0),
                     COALESCE(kcm.metadata, '{{}}'), ks.label, kf.rel_path,
                     bm25({table}) AS rank
              FROM {from_clause}
              {metadata_join} knowledge_chunk_meta kcm ON kcm.chunk_rowid = {rowid}
              JOIN knowledge_files kf ON kf.id = kcm.file_id
              JOIN knowledge_sources ks ON ks.id = kcm.source_id
              WHERE {table} MATCH ? AND kcm.source_id =
                    (SELECT id FROM knowledge_sources WHERE label = ?){range_clause}
              ORDER BY rank LIMIT ?"""


def summarize(samples, result):
    ordered = sorted(samples)
    return {"first_ms": round(samples[0], 3), "p50_ms": round(statistics.median(samples), 3), "p95_ms": round(ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))], 3), "count": len(result)}


def digest(values):
    return hashlib.sha256(json.dumps(values, separators=(",", ":")).encode()).hexdigest()


def benchmark_lexical(path, source, query, layer, repetitions, limit, bounded):
    connection = connect(path)
    source_id, low, high, max_rowid = connection.execute(
        "SELECT s.id, MIN(m.chunk_rowid), MAX(m.chunk_rowid), "
        "(SELECT MAX(chunk_rowid) FROM knowledge_chunk_meta) "
        "FROM knowledge_sources s JOIN knowledge_chunk_meta m ON m.source_id = s.id "
        "WHERE s.label = ? GROUP BY s.id",
        (source,),
    ).fetchone()
    query = bm25_query(query) if layer == "bm25" else identifier_query(query)
    sql = lexical_sql(layer, bounded)
    samples, range_samples, result = [], [], []
    for _ in range(repetitions):
        params = [query, source]
        if bounded:
            range_start = time.perf_counter()
            low, high = connection.execute(
                "SELECT MIN(chunk_rowid), MAX(chunk_rowid) FROM knowledge_chunk_meta "
                "WHERE source_id = (SELECT id FROM knowledge_sources WHERE label = ?)",
                (source,),
            ).fetchone()
            range_samples.append((time.perf_counter() - range_start) * 1000)
            params.extend((low, high))
        params.append(limit)
        start = time.perf_counter()
        result = connection.execute(sql, params).fetchall()
        samples.append((time.perf_counter() - start) * 1000)
    connection.close()
    summary = summarize(samples, result)
    range_p50 = round(statistics.median(range_samples), 3) if range_samples else 0.0
    return {
        "layer": layer,
        "source": source,
        "scope_rowids": [low, high],
        "scope_span_fraction": round((high - low + 1) / max_rowid, 4),
        **summary,
        "range_p50_ms": range_p50,
        "range_plus_query_p50_ms": round(summary["p50_ms"] + range_p50, 3),
        "result_ids": [row[0] for row in result],
        "ranks": [row[-1] for row in result],
    }


def benchmark_coverage(path, repetitions, candidate=False):
    connection = connect(path)
    path_match = "(kf.extension IN (?, ?, ?) OR (kf.extension = '' AND (lower(kf.rel_path) LIKE ? OR lower(kf.rel_path) LIKE ? OR lower(kf.rel_path) LIKE ?)))"
    base = """SELECT COUNT(kcm.chunk_rowid), COUNT(CASE WHEN e.model_id = ? THEN e.chunk_rowid END)
              FROM knowledge_chunk_meta kcm JOIN knowledge_files kf ON kf.id = kcm.file_id
              LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid
              WHERE (lower(kf.rel_path) LIKE ? OR lower(kf.rel_path) LIKE ? OR lower(kf.rel_path) LIKE ?
                     OR lower(json_extract(kcm.metadata, '$.frontmatter.language')) IN (?, ?))"""
    optimized_files = f"""SELECT COALESCE(SUM(kf.chunk_count), 0), COALESCE(SUM(ec.embedded_count), 0)
                         FROM knowledge_files kf LEFT JOIN knowledge_embedding_coverage ec
                           ON ec.file_id = kf.id AND ec.model_id = ? WHERE {path_match}"""
    optimized_metadata = f"""SELECT COUNT(kcm.chunk_rowid),
                              COUNT(CASE WHEN e.model_id = ? THEN e.chunk_rowid END)
                       FROM knowledge_chunk_meta kcm INDEXED BY idx_chunk_meta_language
                       JOIN knowledge_files kf ON kf.id = kcm.file_id
                       LEFT JOIN knowledge_embeddings e ON e.chunk_rowid = kcm.chunk_rowid
                       WHERE CASE WHEN json_valid(kcm.metadata)
                            THEN lower(json_extract(kcm.metadata, '$.frontmatter.language')) END IN (?, ?)
                         AND NOT {path_match}"""
    results = {}
    statements = (
        {
            "candidate_files": (optimized_files, (MODEL, "md", "markdown", "mdx", "%.md", "%.markdown", "%.mdx")),
            "candidate_frontmatter": (optimized_metadata, (MODEL, "markdown", "md", "md", "markdown", "mdx", "%.md", "%.markdown", "%.mdx")),
        }
        if candidate
        else {"baseline": (base, (MODEL, "%.md", "%.markdown", "%.mdx", "markdown", "md"))}
    )
    for name, (sql, params) in statements.items():
        samples, result = [], None
        for _ in range(repetitions):
            start = time.perf_counter()
            result = connection.execute(sql, params).fetchone()
            samples.append((time.perf_counter() - start) * 1000)
        results[name] = {**summarize(samples, result), "coverage": result}
    if candidate:
        results["candidate_combined"] = [sum(results[key]["coverage"][index] for key in ("candidate_files", "candidate_frontmatter")) for index in (0, 1)]
    connection.close()
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, help="baseline knowledge.db path")
    parser.add_argument("--candidate", required=True, help="candidate knowledge.db path")
    parser.add_argument("--source", default="blackhole-renderer")
    parser.add_argument("--query", default="knowledge source file path")
    parser.add_argument("--repetitions", type=int, default=9)
    parser.add_argument("--limit", type=int, default=2000)
    args = parser.parse_args()

    lexical = {}
    for layer in ("bm25", "identifier"):
        bounded_probe = benchmark_lexical(args.candidate, args.source, args.query, layer, args.repetitions, args.limit, True)
        candidate = (
            bounded_probe
            if layer == "bm25"
            else benchmark_lexical(args.candidate, args.source, args.query, layer, args.repetitions, args.limit, False)
        )
        lexical[layer] = {
            "baseline": benchmark_lexical(args.baseline, args.source, args.query, layer, args.repetitions, args.limit, False),
            "candidate": candidate,
        }
        if layer == "identifier":
            lexical[layer]["bounded_probe"] = bounded_probe
        lexical[layer]["same_result_ids"] = lexical[layer]["baseline"]["result_ids"] == lexical[layer]["candidate"]["result_ids"]
        lexical[layer]["same_ranks"] = lexical[layer]["baseline"]["ranks"] == lexical[layer]["candidate"]["ranks"]
        versions = ("baseline", "candidate", "bounded_probe") if layer == "identifier" else ("baseline", "candidate")
        for version in versions:
            result = lexical[layer][version]
            result["result_id_sha256"] = digest(result.pop("result_ids"))
            result.pop("ranks")
    print(json.dumps({"lexical": lexical, "coverage": {"baseline": benchmark_coverage(args.baseline, args.repetitions), "candidate": benchmark_coverage(args.candidate, args.repetitions, True)}}, indent=2))


if __name__ == "__main__":
    main()
