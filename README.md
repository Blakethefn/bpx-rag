# bpx-rag

Local-first retrieval and filesystem analysis for coding agents. bpx-rag indexes project files, finds relevant evidence with lexical and optional semantic search, and returns file/line citations. The connected language model uses that evidence to answer; bpx-rag does not generate answers itself.

It also provides on-demand direct import, include, and link analysis from the current filesystem.

## Quick start

```bash
# Register and index a project directory.
bpx-rag knowledge add /path/to/project --label my-project

# Search indexed code and docs.
bpx-rag search "where is quote status persisted" --source my-project

# Expand the cited chunk from a search result.
bpx-rag knowledge read my-project --path src/quotes/repository.ts --line-start 30 --line-end 55

# Update changed, added, and deleted files.
bpx-rag knowledge sync --label my-project
```

Sources are persisted in a local SQLite knowledge database and incrementally synced using content hashes. Search works immediately with BM25/trigram/fuzzy retrieval. Semantic search becomes available after an embedding model has been downloaded and indexed.

## Embedding models

Model downloads are explicit:

```bash
bpx-rag models status
bpx-rag models download
bpx-rag knowledge embed-backfill
```

The default `jina-embeddings-v2-base-code` model is tuned for code retrieval across 30 programming languages and supports an 8K token input window ([upstream model card](https://huggingface.co/jinaai/jina-embeddings-v2-base-code)). bpx-rag caps each embedded chunk at 1,024 tokens to control local inference cost; the full local model artifacts take about 644 MB. Set `embeddings.model` in `~/.config/bpcontext/config.toml` to `all-MiniLM-L6-v2` for the smaller 91 MB general-purpose baseline, which scored lower MRR and nDCG on the checked-in controlled code benchmark.

Vectors are stored with a canonical model ID. Switching models never mixes vectors from different spaces; run `knowledge embed-backfill` to replace vectors for existing chunks. Search reports incomplete active-model coverage and continues to use lexical results while backfill is pending.

Example model configuration:

```toml
[embeddings]
model = "jina-embeddings-v2-base-code"
enabled = true
```

Embedding batches default to 16 chunks and ONNX uses the available logical CPUs; set `batch_size` or `intra_threads` to tune memory use and throughput. These fields have defaults when omitted from an existing config. The model cache defaults to the local application data directory. `HF_HOME`, when set, selects the Hugging Face cache location.

## CLI

```text
bpx-rag knowledge add <path> --label <label> [--glob <pattern>]
bpx-rag knowledge sync [--label <label>]
bpx-rag knowledge status
bpx-rag knowledge read <label> [--path <relative-path>] [--line-start N] [--line-end N]
bpx-rag search <query> [--source <label>] [--filter <key:value>] [--limit N]
bpx-rag analyze-file <path> [--root <project>]
```

`knowledge read` accepts a relative path and an overlapping 1-based line range from a search citation. This lets agents expand a specific result instead of receiving only the first chunks from a source.

## MCP

```json
{
  "mcpServers": {
    "bpx-rag": {
      "command": "/path/to/bpx-rag",
      "args": ["serve"]
    }
  }
}
```

RAG tools include `bpx_search`, `bpx_knowledge_add`, `bpx_knowledge_sync`, `bpx_knowledge_status`, `bpx_knowledge_read`, and `bpx_embeddings`. `bpx_knowledge_read` accepts the cited source-relative path and line range. `bpx_file_relationships` analyzes the current filesystem and does not need the RAG index.

## Direct file relationships

```bash
bpx-rag analyze-file src/lib/api/quotes.ts --root /path/to/project
```

The analyzer reports direct Markdown wikilinks, imports, includes, and module references with line evidence for Rust, TypeScript/JavaScript, C/C++, C#, Go, Java, Kotlin, Python, PHP, Ruby, Bash, Lua, Dart, and Swift. Static analysis resolves local paths conservatively; it does not infer runtime behavior, symbol-level calls, or build-specific aliases.

## Build and tests

```bash
cargo build --release
cargo test
```

The standard test suite uses deterministic mock embeddings and never downloads a model. Use `bpx-rag models download` when you explicitly want local semantic retrieval.

To run the checked-in judged retrieval benchmark against the configured real model, download it explicitly and then run:

```bash
bpx-rag evaluate retrieval --repetitions 5
bpx-rag evaluate scaling --repetitions 5
# Compare CPU threads and model batch sizes on a real source tree.
bpx-rag evaluate embeddings --corpus-root src --threads 4,8,12 --batch-sizes 16,32 --repetitions 2
# Create a blind review pool and separate rank reveal key.
bpx-rag evaluate pool --dataset eval/bpx-rag-rust --corpus-root src --output /tmp/bpx-rag-pool.json --key-output /tmp/bpx-rag-pool-key.json
# Import completed grades into qrels after reviewing the pool.
bpx-rag evaluate import-pool --pool /tmp/bpx-rag-pool.json --key /tmp/bpx-rag-pool-key.json --dataset eval/bpx-rag-rust --corpus-root src --output /tmp/bpx-rag-reviewed-qrels.jsonl
# Evaluate a judged dataset against a caller-selected real source tree.
bpx-rag evaluate retrieval --dataset eval/bpx-rag-rust --corpus-root src --repetitions 1
```

The retrieval command refuses to download a missing model. It accepts a dataset directory containing `queries.jsonl` and `qrels.jsonl`, plus a corpus root to index; without these flags it uses the bundled fixture. Knowledge search considers up to five times the requested chunk count (capped at 1,000 candidates), fuses chunk-level RRF scores, then fills the requested result limit with the best passage from distinct files before allowing duplicate-file passages. If the corpus returns fewer distinct files than requested, duplicate passages fill the remaining slots, so those fallback slots are not globally score-descending. Reports include hybrid, lexical-only, and vector-only graded metrics and top paths, distinct-file counts, judged coverage at ranks 5 and 10, citation path/line validity with invalid references listed, filter leakage, indexing time, embedding time/call/chunk counts, SQLite write-transaction time, warm-query p50/p95, database/model footprint, query/qrels/corpus hashes, and positive/explicit-negative judgment counts. Precision and nDCG treat unjudged results as nonrelevant; judged-coverage fields show how much of each result list has a human or curated grade. Grades range from 0 (explicitly nonrelevant) to 3 (directly relevant), while a missing grade remains unjudged. Existing queries default to `answerability: "answerable"` for compatibility. An `unanswerable_in_corpus` query must include `answerability_evidence` describing exhaustive corpus review and an explicit grade-0 qrel for every indexable corpus file; any positive or unjudged file rejects the dataset. `evaluate pool` includes every indexed file for these queries so a reviewer can complete that full-corpus judgment. Recall/MRR/nDCG averages include answerable queries only. No-answer reports separately show explicit-negative share, judged coverage, and the fraction of queries where retrieval returned no files. This measures retrieval behavior; whether a connected language model abstains correctly must be evaluated separately.

`evaluate pool` builds a seeded, rank-blinded union of hybrid, lexical, and vector file candidates with relative paths and passage excerpts. It writes the annotator pool and rank-reveal key to separate, new files and refuses to overwrite either. Keep the key private during review. Fill each candidate's `grade` with 0–3 (or leave it `null` if unreviewed), then `evaluate import-pool` validates the pool, key, query file and corpus fingerprints before writing qrels. Pool excerpts may contain source code, so choose local output paths appropriate for the corpus. `eval/bpx-rag-rust/` evaluates the actual `src/` tree with 25 curated self-evaluation queries and 68 graded file judgments; these labels are not independent human annotations. The separate scaling command compares exact vector search with the persistent HNSW candidate index from 1K to 50K synthetic chunks, including index build/load time, query latency, memory, and Recall@10 against exact results. Exact search stays enabled below 4,096 vectors; larger indexes use HNSW candidates followed by SQL metadata rechecks and exact vector reranking. The checked-in reports in `eval/results/` are controlled benchmarks; add independently reviewed, project-specific judgments before using their quality scores as a production claim.

For manual review, open `eval/review-pool.html` in a browser and select the annotator pool JSON only; do not load the reveal key. The page runs offline, displays candidate excerpts as text, supports 0–3 grades, clearing a grade back to unreviewed, optional notes, and keyboard shortcuts (`0`–`3`, `n`/Right, `p`/Left). It warns before discarding changes and downloads a `.reviewed.json` pool that can be passed to `evaluate import-pool`.

`eval/bpx-rag-rust-ai-assessed-v1/` is a 611-qrel diagnostic set graded by blinded model-assisted reviewers, with overlap adjudication; its 71% exact agreement on the 24-candidate overlap is too limited for ground-truth claims. Report: `eval/results/bpx-rag-rust-ai-assisted-v4.json`.

The `bpcontext` executable remains as a compatibility launcher for older scripts.
