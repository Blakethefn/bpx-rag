# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- **Breaking:** commands are flat verbs. `knowledge add|sync|status|remove|read` became `add|sync|status|remove|read`. `knowledge embed-backfill` is now `embed`, `knowledge scrub-sensitive` is now `scrub`, `analyze-file` is now `relate`, and `evaluate` is now `eval`.
- **Breaking:** MCP tools use short names. `bpx_knowledge_add|sync|status|remove|read` are now `bpx_add|sync|status|remove|read`. `bpx_file_relationships` is now `bpx_relate`. `bpx_embeddings` is split into `bpx_models` (status and download) and `bpx_embed` (backfill).
- **Breaking:** the data and config directories are `bpx-rag`, not `bpcontext`. When only the old directory exists, bpx-rag refuses to start with an empty store and prints the `mv` or `ln -s` command.
- Search runs only against the knowledge store. The per-project content index and session memory were removed, along with the `bpcontext` compatibility launcher.
- Source scope: `add` takes repeatable `--include` and `--exclude` globs. The MCP `bpx_add` tool accepts `include` and `exclude` arrays. The old single `glob` still works as one include.
- Top search results list the files they link to or are linked from.
- Source status shows how many chunks still have vectors from another model, and names the `embed` command to fix it.
- Fuzzy matching keeps the content vocabulary in memory and reloads it after the database changes. The first fuzzy query in each process pays for the load: about 30 ms with 100k indexed terms and about 200 ms with 500k. The loaded vocabulary uses about 100 bytes per term. Each misspelled word is compared with about 200 terms at 100k, instead of 500 terms from a fixed sample.

### Fixed

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
