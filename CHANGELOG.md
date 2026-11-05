# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Per-file passage cap. `bpx-rag search --max-per-file N` and the MCP `bpx_search` argument `max_per_file` (1 to 50, default 1) control how many passages one search returns from a single file. The default of 1 is the previous behavior, and the output is identical when the option is unset or set to 1. Larger values allow several passages from one file. When the other files cannot fill the limit, the remaining slots take more passages from the same file, in score order. Zero is rejected. The option applies per query and changes only which passages are selected from the existing candidate pool. Ranking, candidate generation, and citations are unchanged.
- Query embedding cache. A process that searches repeatedly reuses the embedding of a query it has already run, instead of running the model again. The cache is in memory and holds up to `query_cache_entries` vectors under `[embeddings]` (default 256; `0` disables it). It evicts the least recently used entry. Entries are keyed by model and exact query text, so a model change never reuses a vector, and failed or invalid embeddings are never cached. Results, ranking, and diagnostics are unchanged. The benefit is for long-running processes such as `bpx-rag serve` and the MCP server. Each CLI command starts a new process and gets no reuse.
- Search diagnostics. Each search reports what every layer did: `ran` with its candidate count, `skipped` with a reason (for example, the candidate pool was full or the query was under three bytes), or `failed` with an error. `--explain` prints these as a `layers:` line. Intentional skips are never warnings. Ranking is unchanged.
- `eval retrieval` records the layer diagnostics for each query in its case report.
- `eval retrieval --layer-policy` selects the trigram and fuzzy gating policy for evaluation: `current-gating` (the default, and the shipped behavior), `own-pool-gating`, or `unconditional`. Search itself always uses `current-gating`. The alternatives are measured in the local roadmap and are not adopted.
- Watch mode. `bpx-rag watch on|off <label>` turns it on or off for one source (default off, so existing behavior is unchanged). `bpx-rag watch run` runs a watcher in the terminal, and `bpx-rag serve` runs one in the background when a database exists and no other watcher holds it. A relationship-only server creates no database. `bpx-rag watch status` shows whether a watcher is running and each source's state. The MCP tool `bpx_watch` (`on`, `off`, `status`) does the same. A watcher reconciles each source with a full sync when it starts, so changes made while nothing was watching are indexed. After that it reindexes only the files that events name, and it takes the same writer lock as manual sync. A killed watcher is reported as not running, and a restart reconciles again. Only one watcher runs per database.
- Freshness. Each search result shows `freshness: fresh|stale|unknown (basis)`. A file is `fresh` when its size and modification time match the index and the record is not racy, or when its content hash matches. It is `stale` when its content changed (`changed`) or it was deleted (`missing`). It is `unknown` when the source directory is unavailable (`source unavailable`), the file cannot be read (`unreadable`), or the index has no metadata for it yet (`not recorded`). Freshness is checked only for the results that are returned, and a search never writes. A stale result adds a note to the output. Freshness works with watch mode off.
- Symbol lookup. `bpx-rag symbol NAME` (CLI) and the `bpx_symbol` MCP tool find declarations by name across registered sources and return each match's path and line range. Exact match by default; `--prefix` (or `prefix: true`) matches names that start with NAME. Matching is case-sensitive. Duplicate names are all returned. Symbols are extracted from the Tree-sitter parse that chunking already does, for Rust, Python, TypeScript, JavaScript, and Go, and for other grammars whose declarations expose a `name` field. Files with a parse error and files above 2 MiB have no symbols and keep their chunks.

### Changed

- Default search output gains the `freshness:` field on each result line, and a note when a result is stale. Ranking, the result set, and citations are unchanged.
- Sync records each file's modification time before it reads the file's content (`mtime_ns`, `stat_checked_ns`). The first sync after upgrading records these for unchanged files, inside its transaction, with no re-chunking and no re-embedding. Until then those files report `unknown (not recorded)`.
- Source directories are watched only when watch mode is on for them. The `stale_threshold_minutes` and `sync_stale_minutes` settings in the `context` section of `config.toml` are still unused. Stage 7 did not use them and did not remove them.
- Symbol index migration. The first sync after upgrading backfills symbols for each existing source. The backfill reads the files that sync already reads and writes only the symbol rows. It does not re-chunk or re-embed. Until a source syncs, `symbol` reports how many of its files are pending.
- **Breaking:** commands are flat verbs. `knowledge add|sync|status|remove|read` became `add|sync|status|remove|read`. `knowledge embed-backfill` is now `embed`, `knowledge scrub-sensitive` is now `scrub`, `analyze-file` is now `relate`, and `evaluate` is now `eval`.
- **Breaking:** MCP tools use short names. `bpx_knowledge_add|sync|status|remove|read` are now `bpx_add|sync|status|remove|read`. `bpx_file_relationships` is now `bpx_relate`. `bpx_embeddings` is split into `bpx_models` (status and download) and `bpx_embed` (backfill).
- **Breaking:** the data and config directories are `bpx-rag`, not `bpcontext`. When only the old directory exists, bpx-rag refuses to start with an empty store and prints the `mv` or `ln -s` command.
- Search runs only against the knowledge store. The per-project content index and session memory were removed, along with the `bpcontext` compatibility launcher.
- Source scope: `add` takes repeatable `--include` and `--exclude` globs. The MCP `bpx_add` tool accepts `include` and `exclude` arrays. The old single `glob` still works as one include.
- Top search results list the files they link to or are linked from.
- Source status shows how many chunks still have vectors from another model, and names the `embed` command to fix it.
- Fuzzy matching keeps the content vocabulary in memory and reloads it after the database changes. The first fuzzy query in each process pays for the load: about 30 ms with 100k indexed terms and about 200 ms with 500k. The loaded vocabulary uses about 100 bytes per term. Each misspelled word is compared with about 200 terms at 100k, instead of 500 terms from a fixed sample.
- Identifier index. Compound identifiers (`camelCase`, `PascalCase`, `snake_case`, `kebab-case`, and acronym boundaries such as `HTTPServer`) are stored as split words in a new `knowledge_identifiers` table. The first time a database is opened, the table is backfilled from the stored chunk text. This needs no re-indexing and no re-embedding, and existing embeddings are kept.
- Identifier layer. A query whose content words all appear in one chunk's identifier words is reported as the `identifier` layer, in `--explain` output and in the JSON `why.layers`. A single content word does not match. Indexed content is unchanged, and exact identifier search still uses the original content.
- Identifier search checked on an independent prose set (30 queries, 40 documentation files) with no change in ranking, and on a separate identifier set where identifier MRR@10 rose from 0.762 to 0.983. Recall was already at its ceiling on both sets.

### Fixed

- A stored embedding that contains a NaN or infinite value fails the search with an error. The exact vector search used to pass such a score to its sort, which panicked inside the standard library. Finite embeddings are unaffected, and the approximate index already rejected non-finite vectors when it loaded them.
- A failed query embedding is reported in the search output as a warning. Lexical results still return.
- Corrupt knowledge rows fail the query instead of being skipped or defaulted. BM25 and trigram search used to drop any row that failed to decode, and malformed line numbers became `0`. Vector search now rejects an embedding whose length does not match the query dimension, instead of scoring a truncated vector. Malformed chunk metadata, source enrichments, and wikilink metadata during sync are reported as errors.
- Creating the vocabulary table for fuzzy matching no longer ignores its own errors.
- Unknown enrichment names are rejected when a source is added.
- Status no longer double counts chunks that have vectors from more than one model.
- The store directory is created owner-only, and the database file is restricted to its owner on Unix.
- The CUDA wrapper no longer carries machine-specific paths. Those move to the untracked `scripts/bpx-rag-cuda.env`.
- Sync inserts files in path order, so chunk and file rowids no longer depend on hash-map order. Rowids break ties in search, so repeated syncs of the same tree now rank the same way.
- Fuzzy matching reads vocabulary terms in sorted order. Before, the term sample depended on SQLite's scan order, so corrections could change between runs.
- Fuzzy matching searches the whole vocabulary. Before, only the first 500 terms in sorted order were candidates, so a misspelled word whose correct term sorted late (for example `zookeepr`) was not corrected once the index held more than 500 terms.
- Fuzzy matching leaves a query word unchanged when it already matches an indexed term, including when the only difference is case. Before, `Cat` was rewritten to `cat`, which added duplicate fuzzy hits.

### Removed

- Session content index, search-memory learning, and the `bpcontext` launcher binary.
- Unused helpers and the knowledge graph's multi-hop and link-count API. Graph queries use one filtered lookup.

### Internal

- The build has no warnings.
- Code is organized by feature: `search/`, `knowledge/` (with `query/`, `sync/`, `ann/`, `schema/`, `links.rs`, `scope.rs`), `chunk/`, `embed/`, `relations/`, `eval/`, and `mcp/`.
- Inline test modules live in sibling `tests.rs` files.

## [0.1.0] - 2025-04-02

### Added

- CLI with commands: `serve`, `hook`, `execute`, `search`, `index`, `fetch`, `promote`, `stats`, `sources`
- MCP server over JSON-RPC stdio with 8 tools (`bpx_execute`, `bpx_execute_file`, `bpx_batch_execute`, `bpx_search`, `bpx_fetch_and_index`, `bpx_index`, `bpx_promote`, `bpx_stats`)
- SQLite FTS5 indexing with semantic chunking
- Multi-layer search: BM25, trigram, and fuzzy matching
- Head/tail preview generation with configurable ratio
- URL fetching with HTML-to-markdown conversion and caching
- Claude Code hook support (pretooluse, posttooluse, precompact)
- Session-scoped databases with automatic cleanup
- Context savings tracking and stats
- TOML configuration with `--init` scaffolding
- Obsidian/TaskVault integration via `promote` command
