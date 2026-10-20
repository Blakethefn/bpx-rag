# bpx-rag

**Local-first RAG for coding agents.**

bpx-rag builds persistent, searchable knowledge indexes over code and documentation, then returns relevant chunks with file paths, line ranges, and ranking evidence to an MCP-connected coding agent. The agent uses that evidence to answer questions about a project. Search combines full-text, trigram, fuzzy, vector, metadata-filter, and graph signals in a local SQLite store.

The goal is to make project knowledge useful across sessions: ask about a file, API, feature, or design decision and retrieve the source material that supports the answer. bpx-rag runs locally and uses local embeddings, so the index does not require a hosted vector database or embedding API.

## What it does

1. **Indexes project knowledge** — Source files, notes, and docs are chunked and stored in a persistent knowledge database.
2. **Finds evidence** — Hybrid search combines lexical matching, local vector similarity, filters, and available knowledge links.
3. **Returns traceable results** — Results include source paths and line ranges so the agent can inspect the original material.
4. **Supports agent workflows** — MCP tools let coding agents search the index and read selected chunks across sessions.

For example, a coding agent can search for how a quote API is used and inspect the files that mention its types, endpoints, or symbols. Reliable answers about all direct and indirect file interactions require explicit code relationship indexing; the current search index does not yet build a complete dependency graph.

Context saving remains available as a supporting feature: large command results, files, and web pages can be indexed and searched on demand. It is no longer the product's primary purpose.

For the supporting context-saving workflows, full content stays searchable while the agent loads only the relevant pieces.

## Installation

```bash
# CPU only
cargo build --release

# With CUDA support (requires CUDA toolkit)
cargo build --release --features cuda
```

The primary binary is `target/release/bpx-rag`. A `bpcontext` compatibility launcher is also built for existing scripts; it forwards the same arguments to `bpx-rag`. Generate a default config:

```bash
bpx-rag --init
```

Config location remains `~/.config/bpcontext/config.toml` so existing installations keep working.

### First-run model download

On first use, bpx-rag downloads the `all-MiniLM-L6-v2` embedding model (~80MB) from Hugging Face to `~/.local/share/bpcontext/models/`. This is a one-time download — no API keys or accounts needed. If the download fails (e.g., no internet), search falls back to keyword-only mode. The legacy data directory is retained to reuse existing indexes and models.

## Quick Start

### As an MCP Server (recommended)

Add to your Claude Code `.mcp.json`:

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

Then configure Claude Code to use bpx-rag for project retrieval. Add to your `CLAUDE.md`:

```markdown
# bpx-rag Retrieval

- **`bpx_execute`** instead of `Bash` for commands producing >20 lines
- **`bpx_execute_file`** instead of `Read` when analyzing a file (not editing it)
- **`bpx_batch_execute`** instead of multiple Bash/Read/Grep calls when exploring
- **`bpx_fetch_and_index`** instead of `WebFetch` for any URL
```

### As a CLI

```bash
# Run a command and index its output
bpx-rag execute "git log --oneline -50"

# Search indexed content
bpx-rag search "authentication"

# Filter by source or content type
bpx-rag search "error" --source "git log" --content-type code --limit 5

# Index raw text from stdin
echo "some notes" | bpx-rag index "my-notes"

# Fetch and index a web page
bpx-rag fetch https://docs.rs/some-crate

# Check context savings
bpx-rag stats

# List what's been indexed this session
bpx-rag sources

# Backfill embeddings for previously indexed content
bpx-rag embed-backfill

# Show context budget and per-source breakdown
bpx-rag context-status
```

## MCP Tools

| Tool | What it does |
|------|-------------|
| `bpx_execute` | Run a shell command, index the output, return a preview |
| `bpx_execute_file` | Read and index a file with optional processing |
| `bpx_batch_execute` | Run multiple commands + search queries in one call |
| `bpx_search` | Search session index and knowledge store (BM25, trigram, fuzzy, vector similarity, merged via RRF) |
| `bpx_file_relationships` | Scan current files for direct imports, module declarations, and Markdown wikilinks without the knowledge database |
| `bpx_fetch_and_index` | Fetch a URL, convert HTML to markdown, index it |
| `bpx_index` | Index raw text for later search |
| `bpx_index_dir` | Index all files in a directory for the current session |
| `bpx_promote` | Export search results to an Obsidian note via TaskVault |
| `bpx_stats` | Show context savings metrics for the session |
| `bpx_context_status` | Show context budget and per-source breakdown |
| `bpx_read_chunks` | Read specific chunks by ID |

**Knowledge store tools (persistent across sessions):**

| Tool | What it does |
|------|-------------|
| `bpx_knowledge_add` | Register a directory as a persistent knowledge source and run initial sync |
| `bpx_knowledge_sync` | Incrementally re-index changed files across all registered sources |
| `bpx_knowledge_status` | List registered sources, chunk counts, and last sync time |
| `bpx_knowledge_remove` | Unregister a knowledge source and delete all its indexed content |

### On-demand file relationships

Use this for exact questions about what a file links to and what files directly reference it. It reads current project files on each request, respects ignore rules, and returns relative paths and line numbers. It does not require indexing or access the persistent knowledge database.

MCP startup is lazy: listing tools or using `bpx_file_relationships` does not open the content/session databases or load the embedding model. Those initialize only when a database-backed tool is called.

```bash
# Analyze direct imports, module declarations, and reverse references
bpx-rag analyze-file src/lib/api/quotes.ts --root /path/to/project

# The root can be inferred from a nearby project marker
bpx-rag analyze-file src/main.rs
```

The initial analyzer supports Markdown wikilinks, same-crate Rust `mod`/`use` paths, and relative TypeScript/JavaScript imports and re-exports. It does not yet resolve `tsconfig` path aliases or Cargo workspace crate aliases. Results cover direct file relationships; symbol-level calls and transitive runtime effects are not included.

## Persistent Knowledge Store

The knowledge store is a persistent RAG layer that survives across sessions. Unlike the session-scoped index (which is rebuilt each time), the knowledge store registers directories as durable sources and only re-indexes files that have changed (incremental sync via SHA-256 content hash).

This means you can register your project's source directory once and search it in any future session without re-indexing.

### CLI

```bash
# Register a directory as a knowledge source
bpx-rag knowledge add /path/to/project/src --label myproject

# Filter by file type
bpx-rag knowledge add /path/to/docs --label mydocs --glob "**/*.md"

# Enable enrichments (for Obsidian vaults)
bpx-rag knowledge add /path/to/vault --label vault --enrichments frontmatter,wikilinks,folder_tags

# Re-sync all sources (re-indexes changed files only)
bpx-rag knowledge sync

# Re-sync a specific source
bpx-rag knowledge sync --label myproject

# Check status
bpx-rag knowledge status

# Search
bpx-rag knowledge search "authentication flow"

# Remove a source
bpx-rag knowledge remove --label myproject
```

### Session index vs. knowledge store

| | Session index | Knowledge store |
|---|---|---|
| Lifetime | Current session only | Persistent across sessions |
| How populated | `bpx_execute`, `bpx_index`, `bpx_index_dir` | `bpx_knowledge_add` / `bpx-rag knowledge add` |
| Re-index cost | Full re-index every session | Incremental — only changed files |
| Use case | One-off exploration | Frequently referenced codebases and docs |

`bpx_search` queries **both** layers simultaneously and merges results via RRF, so you get session-context results and persistent knowledge in a single call.

### Enrichments

Enrichments extract structured metadata at index time for richer filtering:

| Enrichment | What it extracts |
|---|---|
| `frontmatter` | YAML frontmatter fields (e.g., `status`, `type`, `tags`) |
| `wikilinks` | Outgoing `[[wikilinks]]` from Obsidian notes |
| `folder_tags` | Parent folder names as implicit tags |

Enrichments are stored alongside each chunk and available for metadata filtering in search results.

### Data storage

The knowledge store uses a single global database separate from per-session content databases:

- **Knowledge DB:** `~/.local/share/bpcontext/knowledge.db` — registered sources, file hashes, chunks, and embeddings

## Claude Code Hooks

bpx-rag can also run as Claude Code hooks to automatically intercept tool output:

- **pretooluse** — Intercepts before tool execution
- **posttooluse** — Processes and compresses tool output after execution
- **precompact** — Runs before context compaction

## Semantic Search

bpx-rag adds vector similarity using local embeddings alongside lexical search.

- **Model:** `sentence-transformers/all-MiniLM-L6-v2` (384 dimensions, ~80MB)
- **Inference:** [Candle](https://github.com/huggingface/candle) (Rust ML framework) — runs on CPU by default, CUDA optional
- **At index time:** each chunk is embedded and stored as a BLOB in SQLite
- **At search time:** the query is embedded and compared against all stored vectors via dot product (brute-force, <1ms for typical session sizes)
- **Fusion:** vector results are merged into the existing RRF (Reciprocal Rank Fusion) pipeline alongside BM25, trigram, and fuzzy results

This lets `bpx_search` find conceptually related chunks even when they do not share the query's exact wording. It is retrieval evidence for the connected agent; generation and answer grounding are handled by that agent.

The current index is chunk- and metadata-based. It does not yet construct a complete static dependency graph for questions such as “which files directly and indirectly interact with this source file?” That requires code relationship indexing in addition to RAG search.

Weights are configurable:
```toml
[search]
vector_weight = 1.0    # multiplier for semantic results
keyword_weight = 1.0   # multiplier for keyword results
```

## Context Manager

bpx-rag tracks returned content and can surface context-budget advice inline as a supporting feature.

**How it works:** after every tool response, the context ledger checks utilization against the configured budget. When a threshold is crossed, an alert is appended to the response:

| Utilization | What the agent sees |
|-------------|---------------------|
| 40% | Token count and source count |
| 60% | Top consumers, nudge toward search |
| 70% | Stale sources to drop, relevance scores |
| 80% | Explicit compact recommendation with keep/drop lists |
| 90% | Critical alert with remaining tokens |

Each threshold fires once per session. The precompact hook also includes keep/drop recommendations.

## Configuration

```toml
[general]
max_stdout_bytes = 102400   # max bytes captured per command
head_ratio = 0.6            # fraction of preview from head vs tail

[search]
default_limit = 10
throttle_max = 8            # max searches per window
throttle_window_secs = 60
vector_weight = 1.0         # weight for semantic search in RRF fusion
keyword_weight = 1.0        # weight for keyword search in RRF fusion

[fetch]
cache_ttl_hours = 24        # cache fetched URLs

[embeddings]
model = "all-MiniLM-L6-v2"
model_dir = "~/.local/share/bpcontext/models" # legacy-compatible data directory
batch_size = 32
enabled = true              # set to false to disable embeddings entirely

[context]
budget_tokens = 200000      # estimated context window budget
stale_threshold_minutes = 30

[integration]
taskvault_bin = "/path/to/taskvault"  # optional: for promote command
vault_path = "/path/to/obsidian_docs" # optional: Obsidian vault path

[cleanup]
stale_db_days = 14          # auto-cleanup old session databases
```

## Architecture

```
src/
  cli.rs          — CLI argument parsing (clap)
  config.rs       — TOML config loading
  db.rs           — SQLite connection and session DB management
  fetch.rs        — URL fetching and HTML-to-markdown conversion
  indexdir.rs     — Directory indexing for bpx_index_dir
  promote.rs      — Export results to Obsidian via TaskVault
  stats.rs        — Context savings tracking
  truncate.rs     — Head/tail preview generation
  context/        — Smart context manager (ledger, relevance scoring, alerts)
  embedder/       — Local embedding model (Candle + all-MiniLM-L6-v2)
  executor/       — Command execution and output capture
  hooks/          — Claude Code hook handlers (pre/post tool use, precompact)
  knowledge/      — Persistent knowledge store (RAG): source registry, incremental sync, enrichments, search
  mcp/            — MCP server (JSON-RPC over stdio)
  session/        — Session lifecycle and event tracking
  store/          — Chunking, FTS5 indexing, and multi-layer search (BM25 + trigram + fuzzy + vector)
```

## Data Storage

All runtime data is stored under XDG-standard directories:

- **Content DBs:** `~/.local/share/bpcontext/content/{hash}.db` — per-project FTS5 indexes + embeddings (session-scoped; legacy-compatible path)
- **Session DBs:** `~/.local/share/bpcontext/sessions/{hash}.db` — events + context ledger
- **Knowledge DB:** `~/.local/share/bpcontext/knowledge.db` — persistent knowledge store (source registry, file hashes, chunks, embeddings; legacy-compatible path)
- **Models:** `~/.local/share/bpcontext/models/` — downloaded embedding weights
- **Config:** `~/.config/bpcontext/config.toml` — retained for compatibility

## Requirements

- Rust 1.70+
- SQLite (bundled via rusqlite)
- For CUDA: CUDA toolkit 12.0+ and a compatible GPU

## License

MIT
