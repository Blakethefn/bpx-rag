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
# Purge already-indexed files matched by the sensitive path/content policy.
bpx-rag knowledge scrub-sensitive [--label my-project]
```

Sources are persisted in a local SQLite knowledge database and incrementally synced using content hashes and per-file chunker versions. Supported source files use the 17 bundled Tree-sitter code grammars for Rust, JavaScript/TypeScript/TSX, C/C++, C#, Go, Java, Python, PHP, Ruby, Bash, Lua, Dart, Swift, and Kotlin. Tree-sitter declaration boundaries keep functions and related syntax together; Markdown, unsupported extensions, parse-error files, and source files over 2 MiB use the existing chunker. Chunks remain capped at 4 KiB. When a supported code file is first encountered after upgrading, its stored legacy chunker version causes it to be reindexed on the next sync even if its content is unchanged. Sync summaries and evaluation reports show Tree-sitter and existing-chunker file/chunk counts for files indexed during that operation; the embedding benchmark reports the same split for its corpus. Search works immediately with BM25/trigram/fuzzy retrieval. Semantic search becomes available after an embedding model has been downloaded and indexed. The source walker skips VCS metadata directories (`.git`, `.hg`, `.svn`), common credential-store directories (`.aws`, `.azure`, `.gnupg`, `.kube`, `.ssh`, `.terraform`, `credentials`, `secrets`), common credential files, `.env` files other than names ending in `.example`, `.sample`, `.template`, `.dist`, or `.defaults`, key/certificate containers (`.key`, `.pem`, `.p12`, `.pfx`, `.jks`, `.keystore`), and Terraform state—even when those paths are not in `.gitignore`. It also skips files matching a high-confidence Google API-key signature. `knowledge scrub-sensitive` removes already-indexed rows matched by the path/name policy or the recognized key signature, without walking or indexing source trees; it reports aggregate counts only. Sync and scrub summaries never list skipped paths or contents. Sample-file exceptions are based on the filename suffix and still require placeholder contents; benign `.npmrc`, `.pypirc`, or PEM certificate files are also excluded by this conservative rule. These are targeted safeguards, not comprehensive secret scanning; other secret formats may still be indexed.

## Embedding models

Model downloads are explicit:

```bash
bpx-rag models status
bpx-rag models download
bpx-rag knowledge embed-backfill
# Backfill one registered source at a time for large knowledge stores.
bpx-rag knowledge embed-backfill --label my-project
```

The default `jina-embeddings-v2-base-code` model is tuned for code retrieval across 30 programming languages and supports an 8K token input window ([upstream model card](https://huggingface.co/jinaai/jina-embeddings-v2-base-code)). bpx-rag caps each embedded chunk at 1,024 tokens to control local inference cost; the full local model artifacts take about 644 MB. Set `embeddings.model` in `~/.config/bpcontext/config.toml` to `all-MiniLM-L6-v2` for the smaller 91 MB general-purpose baseline, which scored lower MRR and nDCG on the checked-in controlled code benchmark.

Vectors are stored with a canonical model ID. Switching models never mixes vectors from different spaces; run `knowledge embed-backfill` to replace vectors for existing chunks. Use `--label <source-label>` to backfill just one registered source; omitting it retains all-source behavior. Backfill processes bounded batches, rechecks chunks before writing to account for concurrent syncs/removals, and reports remaining chunks for the active model. Search reports incomplete active-model coverage and continues to use lexical results while backfill is pending. The `bpx_embeddings` MCP tool accepts the same optional `label` with `action: "backfill"`.

Example model configuration:

```toml
[embeddings]
model = "jina-embeddings-v2-base-code"
enabled = true
device = "cpu"
```

Embedding batches default to 16 chunks and ONNX uses the available logical CPUs; set `batch_size` or `intra_threads` to tune memory use and throughput. These fields have defaults when omitted from an existing config. The model cache defaults to the local application data directory. `HF_HOME`, when set, selects the Hugging Face cache location.

### Optional NVIDIA CUDA inference

CPU is the default. To enable CUDA, build with `--features gpu-cuda`, set `embeddings.device = "cuda"`, and make the CUDA and cuDNN runtime libraries discoverable by the process. The CUDA execution provider is configured to fail clearly when it cannot register. Reports from `evaluate embeddings` and `evaluate retrieval` record the selected device.

The pinned ONNX Runtime CUDA provider requires CUDA 13.2 or newer and cuDNN 9.23 or newer. Set `ORT_CACHE_DIR` to a location on the desired data drive when building. On this workstation, the ignored local `.cargo/config.toml` points it to `/media/blakethefn/2000HDD1/dev/project-assets/bpcontext/ort-cache`. Run CUDA-enabled commands through `scripts/bpx-rag-cuda`; it exposes the local CUDA/cuDNN libraries from the same HDD path. Existing configurations without a `device` field continue to use CPU.

Build the optional provider with:

```sh
cargo build --release --features gpu-cuda
```

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

The `language:<name>` filter matches source-file extensions for common languages as well as an optional YAML frontmatter `language` value. For example, `--filter language:rust` works on ordinary `.rs` files without requiring frontmatter or reindexing existing sources. Registered aliases include names such as `c++`/`cpp` and `c#`/`csharp`; unregistered safe names such as `nix` or `tf` match that extension. `.h` is intentionally included in both C and C++ filters because the extension is ambiguous.

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
bpx-rag evaluate embeddings --corpus-root src --glob '**/*' --threads 4,8,12 --batch-sizes 16,32 --repetitions 2
# Create a blind review pool and separate rank reveal key.
bpx-rag evaluate pool --dataset eval/bpx-rag-rust --corpus-root src --output /tmp/bpx-rag-pool.json --key-output /tmp/bpx-rag-pool-key.json
# Import completed grades into qrels after reviewing the pool.
bpx-rag evaluate import-pool --pool /tmp/bpx-rag-pool.json --key /tmp/bpx-rag-pool-key.json --dataset eval/bpx-rag-rust --corpus-root src --output /tmp/bpx-rag-reviewed-qrels.jsonl
# Evaluate a judged dataset against a caller-selected real source tree.
bpx-rag evaluate retrieval --dataset eval/bpx-rag-rust --corpus-root src --repetitions 1
# Exploratory hybrid-fusion comparison; defaults are 1:1.
bpx-rag evaluate retrieval --dataset eval/bpx-rag-rust-ai-assessed-v1 --corpus-root src --repetitions 1 --keyword-weight 1 --vector-weight 2
```

`evaluate retrieval` accepts `--keyword-weight` and `--vector-weight` to compare hybrid reciprocal-rank-fusion settings; both default to `1.0` and must be finite in `(0, 10]`. Each report records the effective values. Treat weight sweeps over model-assisted or incomplete qrels as exploratory diagnostics, not grounds for changing production defaults.

`evaluate embeddings` accepts the same optional `--glob` as source registration so CPU/GPU throughput comparisons can use the registered source scope. Reports record the applied glob.

The retrieval command refuses to download a missing model. It accepts a dataset directory containing `queries.jsonl` and `qrels.jsonl`, plus a corpus root to index; without these flags it uses the bundled fixture. Knowledge search considers up to five times the requested chunk count (capped at 1,000 candidates), fuses chunk-level RRF scores, then fills the requested result limit with the best passage from distinct files before allowing duplicate-file passages. If the corpus returns fewer distinct files than requested, duplicate passages fill the remaining slots, so those fallback slots are not globally score-descending. Reports include hybrid, lexical-only, and vector-only graded metrics and top paths, distinct-file counts, judged coverage at ranks 5 and 10, citation path/line validity plus a separate check that each returned excerpt matches the beginning of its cited source lines, filter leakage, indexable corpus and sensitive-file skip counts, Tree-sitter/existing-chunker file and chunk counts, indexing time, embedding time/call/chunk counts, SQLite write-transaction time, warm-query p50/p95, database/model footprint, query/qrels/corpus hashes, and positive/explicit-negative judgment counts. The excerpt/source match rate is `null` when no citations were returned. Path/line or excerpt/source citation failures make the evaluation command fail. Precision and nDCG treat unjudged results as nonrelevant; judged-coverage fields show how much of each result list has a human or curated grade. Grades range from 0 (explicitly nonrelevant) to 3 (directly relevant), while a missing grade remains unjudged. Existing queries default to `answerability: "answerable"` for compatibility. An `unanswerable_in_corpus` query must include `answerability_evidence` describing exhaustive corpus review and an explicit grade-0 qrel for every indexable corpus file; any positive or unjudged file rejects the dataset. `evaluate pool` includes every indexed file for these queries so a reviewer can complete that full-corpus judgment. Recall/MRR/nDCG averages include answerable queries only. No-answer reports separately show explicit-negative share, judged coverage, and the fraction of queries where retrieval returned no files. This measures retrieval behavior; whether a connected language model abstains correctly must be evaluated separately.

`evaluate pool` builds a seeded, rank-blinded union of hybrid, lexical, and vector file candidates with relative paths and passage excerpts. It writes the annotator pool and rank-reveal key to separate, new files and refuses to overwrite either. Keep the key private during review. Fill each candidate's `grade` with 0–3 (or leave it `null` if unreviewed), then `evaluate import-pool` validates the pool, key, query file and corpus fingerprints before writing qrels. Pool excerpts may contain source code, so choose local output paths appropriate for the corpus. `eval/bpx-rag-rust/` evaluates the actual `src/` tree with 25 curated self-evaluation queries and 68 graded file judgments; these labels are not independent human annotations. The separate scaling command compares exact vector search with the persistent HNSW candidate index from 1K to 50K synthetic chunks, including index build/load time, query latency, memory, and Recall@10 against exact results. Exact search stays enabled below 4,096 vectors; larger indexes use HNSW candidates followed by SQL metadata rechecks and exact vector reranking. The checked-in reports in `eval/results/` are controlled benchmarks; add independently reviewed, project-specific judgments before using their quality scores as a production claim.

For manual review, open `eval/review-pool.html` in a browser and select the annotator pool JSON only; do not load the reveal key. The page runs offline, displays candidate excerpts as text, supports 0–3 grades, clearing a grade back to unreviewed, optional notes, and keyboard shortcuts (`0`–`3`, `n`/Right, `p`/Left). It warns before discarding changes and downloads a `.reviewed.json` pool that can be passed to `evaluate import-pool`.

`eval/bpx-rag-rust-ai-assessed-v1/` is a 611-qrel diagnostic set graded by blinded model-assisted reviewers, with overlap adjudication; its 71% exact agreement on the 24-candidate overlap is too limited for ground-truth claims. Report: `eval/results/bpx-rag-rust-ai-assisted-v4.json`.

The bundled `eval/retrieval/` multilingual synthetic fixture has a complete 34-query × 35-file matrix (1,190 qrels, including 1,131 explicit negatives). Its labels are author-curated, not independently human-reviewed; use its 100% judged coverage for deterministic regression analysis, not production-quality relevance claims.

With syntax-aware chunking, that fixture indexed 35 files / 37 chunks; 4 files used Tree-sitter and 31 used the existing chunker fallback. Recall@10 and MRR@10 were unchanged from the pre-Tree-sitter run; nDCG@10 moved from 0.8549 to 0.8552. The comparison uses self-curated synthetic qrels, so it is a regression diagnostic rather than independent evidence. The prior report is retained as `eval/results/retrieval-multilingual-coverage-pre-tree-sitter-2026-10-21.json`.

The citation-content check matched all 331 returned excerpts against their cited source lines in the 34-query multilingual fixture. Its report is `eval/results/retrieval-citation-content-multilingual-2026-10-21.json`.

The current Rust source run matched all 250/250 returned excerpts against their cited lines across 55 files / 630 chunks. Its 25-query qrels remain self-authored with 22.4% judged coverage@10, so the ranking scores are development evidence only. Report: `eval/results/bpx-rag-rust-citation-content-2026-10-21.json`.

The `bpcontext` executable remains as a compatibility launcher for older scripts.
