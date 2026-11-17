#!/usr/bin/env python3
"""Compare baseline and candidate search output on paired database snapshots."""

import argparse
import hashlib
import json
import os
import subprocess

QUERIES = [
    ("directory", "knowledge source file path", "dir:src/knowledge/query/lexical"),
    ("exact_file", "BM25 lexical query ranking scope", "file:src/knowledge/query/lexical/bm25.rs"),
    ("language", "search filtering markdown language", "language:markdown"),
    ("tag_language", "implementation retrieval markdown", "tag:implementation language:markdown"),
    ("frontmatter", "hierarchical scoped retrieval task", "source:bpx-rag type:task dir:src"),
]


def search(binary, data_home, config_home, hf_home, label, text, filter_value):
    env = os.environ.copy()
    env.update(
        {
            "XDG_DATA_HOME": data_home,
            "XDG_CONFIG_HOME": config_home,
            "HF_HOME": hf_home or f"{config_home}/missing-model-cache",
        }
    )
    completed = subprocess.run(
        [binary, "search", text, "--filter", filter_value, "--limit", "20", "--explain"],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )
    if completed.returncode:
        raise RuntimeError(f"{label} search failed with exit {completed.returncode}")
    return completed.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-bin", required=True)
    parser.add_argument("--candidate-bin", required=True)
    parser.add_argument("--baseline-data", required=True)
    parser.add_argument("--candidate-data", required=True)
    parser.add_argument("--config-home", required=True)
    parser.add_argument("--hf-home")
    args = parser.parse_args()
    results = []
    for query_id, text, filter_value in QUERIES:
        baseline = search(args.baseline_bin, args.baseline_data, args.config_home, args.hf_home, query_id, text, filter_value)
        candidate = search(args.candidate_bin, args.candidate_data, args.config_home, args.hf_home, query_id, text, filter_value)
        results.append(
            {
                "query_id": query_id,
                "filter": filter_value,
                "same_output": baseline == candidate,
                "baseline_sha256": hashlib.sha256(baseline.encode()).hexdigest(),
                "candidate_sha256": hashlib.sha256(candidate.encode()).hexdigest(),
            }
        )
    print(json.dumps({"query_count": len(results), "all_equal": all(row["same_output"] for row in results), "queries": results}, indent=2))


if __name__ == "__main__":
    main()
