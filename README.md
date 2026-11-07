# bpx-rag

Local-first retrieval and filesystem analysis for coding agents. bpx-rag indexes project files, finds relevant evidence with lexical and optional semantic search, and returns file/line citations. The connected language model uses that evidence to answer; bpx-rag does not generate answers itself.

It also provides on-demand direct import, include, and link analysis from the current filesystem.

## Quick start

```bash
# Register and index a project directory.
bpx-rag add /path/to/project --label my-project

# Search indexed code and docs.
bpx-rag search "where is quote status persisted" --source my-project

# Expand the cited chunk from a search result.
bpx-rag read my-project --path src/quotes/repository.ts --line-start 30 --line-end 55

# Update changed, added, and deleted files.
bpx-rag sync --label my-project
# Purge already-indexed files matched by the sensitive path/content policy.
bpx-rag scrub [--label my-project]
```

Sources are persisted in a local SQLite knowledge database and incrementally synced using content hashes and per-file chunker versions. RAG chunking has bundled Tree-sitter grammars for 29 source languages: Rust, JavaScript/TypeScript/TSX, C/C++, C#, Go, Java, Python, PHP, Ruby, Bash, Lua, Dart, Swift, Kotlin, SQL, Scala, Haskell, Elixir, Erlang, R, Julia, Perl, PowerShell, Solidity, HTML, and CSS. Tree-sitter declaration boundaries keep functions and related syntax together; Markdown, unsupported extensions, parse-error files, and source files over 2 MiB use the existing content chunker. Chunks remain capped at 4 KiB. When a supported code file is first encountered after upgrading, its stored legacy chunker version causes it to be reindexed on the next sync even if its content is unchanged. Sync summaries report Tree-sitter and existing-chunker file/chunk counts for files indexed during that operation. Search works immediately with BM25/trigram/fuzzy retrieval. Semantic search becomes available after an embedding model has been downloaded and indexed. The source walker skips VCS metadata directories (`.git`, `.hg`, `.svn`), common credential-store directories (`.aws`, `.azure`, `.gnupg`, `.kube`, `.ssh`, `.terraform`, `credentials`, `secrets`), common credential files, `.env` files other than names ending in `.example`, `.sample`, `.template`, `.dist`, or `.defaults`, key/certificate containers (`.key`, `.pem`, `.p12`, `.pfx`, `.jks`, `.keystore`), and Terraform state—even when those paths are not in `.gitignore`. It also skips files matching a high-confidence Google API-key signature. `scrub` removes already-indexed rows matched by the path/name policy or the recognized key signature, without walking or indexing source trees; it reports aggregate counts only. Sync and scrub summaries never list skipped paths or contents. Sample-file exceptions are based on the filename suffix and still require placeholder contents; benign `.npmrc`, `.pypirc`, or PEM certificate files are also excluded by this conservative rule. These are targeted safeguards, not comprehensive secret scanning; other secret formats may still be indexed.

Two kinds of file are skipped with a reason instead of failing the sync: text files that are not valid UTF-8 (`not valid UTF-8: ...`), and files that cannot be opened or read, such as a permission failure or an I/O error (`unreadable: Permission denied (os error 13)`). The sync summary and `watch status` name each skipped file with its reason, and the other files sync normally. If an indexed file becomes invalid or unreadable, its rows are removed, because the index can no longer be checked against it; it is indexed again once it can be read. A failure of the directory walk itself, such as a directory that cannot be listed, still fails the sync, because the files inside it are unknown and skipping them would remove their rows. Files with a null byte are still treated as binary and are not reported.

## Embedding models

Model downloads are explicit:

```bash
bpx-rag models status
bpx-rag models download
bpx-rag embed
# Backfill one registered source at a time for large knowledge stores.
bpx-rag embed --label my-project
```

The default `jina-embeddings-v2-base-code` model is tuned for code retrieval across 30 programming languages and supports an 8K token input window ([upstream model card](https://huggingface.co/jinaai/jina-embeddings-v2-base-code)). bpx-rag caps each embedded chunk at 1,024 tokens to control local inference cost; the full local model artifacts take about 644 MB. Set `embeddings.model` in `~/.config/bpx-rag/config.toml` to `all-MiniLM-L6-v2` to use the smaller 91 MB general-purpose model.

Vectors are stored with a canonical model ID. Switching models never mixes vectors from different spaces; run `embed` to replace vectors for existing chunks. Use `--label <source-label>` to backfill just one registered source; omitting it retains all-source behavior. Backfill processes bounded batches, rechecks chunks before writing to account for concurrent syncs/removals, and reports remaining chunks for the active model. Search reports incomplete active-model coverage and continues to use lexical results while backfill is pending. The `bpx_embed` MCP tool accepts the same optional `label`.

Example model configuration:

```toml
[embeddings]
model = "jina-embeddings-v2-base-code"
enabled = true
device = "cpu"
```

Embedding batches default to 16 chunks and ONNX uses the available logical CPUs; set `batch_size` or `intra_threads` to tune memory use and throughput. These fields have defaults when omitted from an existing config. The model cache defaults to the local application data directory. `HF_HOME`, when set, selects the Hugging Face cache location. `query_cache_entries` (default 256; `0` disables) keeps recent query embeddings in memory so that a long-running process skips the model for repeated queries.

### Optional NVIDIA CUDA inference

CPU is the default. To enable CUDA, build with `--features gpu-cuda`, set `embeddings.device = "cuda"`, and make the CUDA and cuDNN runtime libraries discoverable by the process. The CUDA execution provider is configured to fail clearly when it cannot register.

The pinned ONNX Runtime CUDA provider requires CUDA 13.2 or newer and cuDNN 9.23 or newer. Set `ORT_CACHE_DIR` to a location on a drive with room for the provider binaries when building, for example in the ignored `.cargo/config.toml`. Run CUDA-enabled commands through `scripts/bpx-rag-cuda`. It reads the CUDA runtime directory from `BPX_RAG_CUDA_RUNTIME_DIR` and the binary from `BPX_RAG_BINARY`. Put those machine-specific paths in the untracked `scripts/bpx-rag-cuda.env`. Existing configurations without a `device` field continue to use CPU.

Build the optional provider with:

```sh
cargo build --release --features gpu-cuda
```

**A configuration with `device = "cuda"` needs a `gpu-cuda` build.** A build without the feature cannot honor it. Searches still succeed and exit 0, but the embedding model does not load, so retrieval silently becomes keyword-only (BM25, trigram, fuzzy, and identifier layers, with no vector layer). The only signs are a trailing `Embedding status:` note that says `CUDA was selected, but this binary lacks CUDA support; rebuild with `--features gpu-cuda``, and `device: cuda requested, but this binary was built without `gpu-cuda`` in `bpx-rag models status`. `bpx-rag search --explain` puts a `Warning: vector retrieval is not running ...` line first, and its `layers:` line says `vector skipped (no embedder attached)` where a working build says `vector ran`. Before you point a client at a new binary, run `search --explain` with the production config and confirm the `layers:` line shows `vector ran`. A CPU build that reads a CUDA config is a deployment error, not a supported fallback; set `device = "cpu"` for CPU builds.

## CLI

```text
bpx-rag serve                                   start the MCP server on stdio
bpx-rag search <query> [--source <label>] [--filter <key:value>] [--limit N] [--max-per-file N]
bpx-rag add <path> --label <label> [--include <glob>]... [--exclude <glob>]...
bpx-rag sync [--label <label>]
bpx-rag status
bpx-rag remove <label>
bpx-rag read <label> [--path <relative-path>] [--line-start N] [--line-end N]
bpx-rag embed [--label <label>]                 compute missing vectors
bpx-rag scrub [--label <label>]                 delete rows matching the sensitive-data policy
bpx-rag relate <path> [--root <project>]        direct imports and links, no index needed
bpx-rag symbol <name> [--prefix]               find declarations by name
bpx-rag watch on|off <label> | status | run   keep sources in sync with the filesystem
bpx-rag models status|download
bpx-rag eval retrieval|scaling|embeddings|pool|import-pool
```

`read` accepts a relative path and an overlapping 1-based line range from a search citation. This lets agents expand a specific result instead of receiving only the first chunks from a source.

### Index only part of a project

`add` takes repeatable `--include` and `--exclude` globs, so a source can cover a subtree and skip what does not matter:

```bash
bpx-rag add ~/code/api --label api --include 'src/**' --include 'docs/**' --exclude '**/*.test.ts'
```

A scope is set when a source is added. Invalid patterns fail immediately. The sensitive-data policy still applies to everything in scope.

Each top search result lists the files it links to or is linked from, as `related:` under the hit. Wikilink and frontmatter edges provide these links.

By default each search returns at most one passage per file, so the results cover as many files as possible. To read several passages from one file, pass `--max-per-file N` on the CLI or `max_per_file` to `bpx_search` (1 to 50, default 1). The first pass keeps the best N passages of each file, in score order, until the limit is reached. If the other files do not supply enough passages to reach the limit, the remaining slots are filled with the next-best passages, including more from the same file. A cap is therefore a preference that applies while other files can fill the results. Output is ordered by score, as before. Ranking, candidate generation, and citations are unchanged.

The `language:<name>` filter matches source-file extensions for common languages as well as an optional YAML frontmatter `language` value. For example, `--filter language:rust` works on ordinary `.rs` files without requiring frontmatter or reindexing existing sources. Registered aliases include names such as `c++`/`cpp` and `c#`/`csharp`; unregistered safe names such as `nix` or `tf` match that extension. `.h` is intentionally included in both C and C++ filters because the extension is ambiguous.

## Freshness

Each search result shows `freshness: <state> (<basis>)`. The basis says how the state was decided:

- `fresh (metadata match, content not hashed)`: the file's size and modification time match the index. This is a fast check, not proof that the content is unchanged. An edit that keeps the size and restores the modification time is reported as fresh until the next sync, which compares content hashes.
- `fresh (hash)`: the content hash matched the index. This is verified.
- `stale (changed)` or `stale (missing)`: the content changed or the file was deleted since the last sync.
- `unknown (...)`: the source directory is unavailable, the file cannot be read, or the index has no metadata for it yet.

Freshness is computed only for returned results. Searching does not hash files. Run `bpx-rag sync` to re-verify a source by content hash.

### Searching and the database write lock

A search does not change indexed content, but opening the database is not always read-only:

- Opening an up-to-date database takes no write lock, so a search is not blocked while a sync or the watcher holds one.
- An open that has work to do takes SQLite's write lock: a schema or index migration, or identifier rows queued by chunks that an older build wrote. A search drains that queue, so the first search or open after such a write indexes those chunks. If another process holds the write lock for longer than the 5 second busy timeout, that search fails with `database is locked`.
- Fuzzy matching's vocabulary table is created with the schema, not on the first fuzzy search.

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

RAG tools include `bpx_search`, `bpx_add` (with `include` and `exclude` arrays), `bpx_sync`, `bpx_status`, `bpx_remove`, `bpx_read`, `bpx_embed`, and `bpx_models` (status and download). Earlier builds named these `bpx_knowledge_add`, `bpx_knowledge_sync`, `bpx_knowledge_status`, `bpx_knowledge_remove`, `bpx_knowledge_read`, `bpx_embeddings`, and `bpx_file_relationships` (now `bpx_relate`); update client configuration that uses the old names. `bpx_read` accepts the cited source-relative path and line range. `bpx_watch` (CLI: `watch on|off|status|run`) controls watch mode per source, which keeps a source in sync as its files change. A watcher runs in one server or terminal per database. `bpx_symbol` (CLI: `symbol`) finds declarations by exact name, or by prefix with `prefix`, and returns every path and line range that matches. `bpx_relate` analyzes the current filesystem and does not need the RAG index.

## Direct file relationships

```bash
bpx-rag relate src/lib/api/quotes.ts --root /path/to/project
```

The analyzer reports direct Markdown wikilinks, imports, includes, and module references with line evidence for Rust, TypeScript/JavaScript, C/C++, C#, Go, Java, Kotlin, Scala, Haskell, Erlang, Elixir, R, Julia, Perl, PowerShell, Solidity, Python, PHP, Ruby, Bash, Lua, Dart, Swift, HTML, and CSS. SQL has syntax-aware RAG chunking but no file-import extraction. Static analysis resolves local paths conservatively; it does not infer runtime behavior, symbol-level calls, or build-specific aliases. External packages and web URLs are not treated as local files.

Syntax-aware Tree-sitter chunking supports 29 source grammars. Parse-error and oversized files retain the existing chunker.

## Platform support and watch limitations

`bpx-rag` is developed and tested on Linux. Watch mode uses Linux inotify events through the `notify` crate, and it has only been validated on local filesystems. It has these limits:

- Changes made on another machine, or through a network or FUSE mount that does not deliver local filesystem events, are not seen. This includes NFS, SMB/CIFS, and Windows-mounted drives. Watch mode reconciles each source with a full sync when it starts, but it misses later edits made outside the local kernel. Run `bpx-rag sync` on such sources, or leave watch mode off for them.
- inotify has per-user limits on instances (`fs.inotify.max_user_instances`, default 128) and watches (`fs.inotify.max_user_watches`). Desktop applications and editors share the instance limit. When it is used up, or a tree exceeds the watch limit, registration fails with `watch registration failed`, for example `Too many open files`. The source stays in an error state and is retried with backoff. `watch status` shows the error.
- The writer lock and the watcher lease use `flock`. Network filesystems may not enforce it, so keep the data directory on a local filesystem.
- Only one watcher runs per database.
- Watch mode is off by default for every source. `bpx-rag watch on <label>` enables it for one source.

## Upgrading an existing database

Before the first open with this version, stop every `bpx-rag` process that uses the database. That includes long-running `serve` processes and `watch run`, and any CLI command. Back up `knowledge.db` first.

The first open rebuilds the identifier index once, with one row per chunk. Chunk rows, embeddings, and the embedding generation are not changed, and no file is re-chunked or re-embedded. The rebuild holds the database's write lock, so other writers wait or fail with a busy error until it finishes. It took 4.1 seconds for 98,092 chunks in a release build on the audit machine (a debug build is several times slower). Ordinary opens afterward add about 0.1 seconds.

Processes that predate this version can keep using the database afterward. Their writes are still covered by database triggers. A current process that was already running when an older build wrote chunks does not match them in the identifier layer until it reopens the database or syncs. Stop older processes anyway, so the rebuild is not repeated against live writers.

### Symbol lookup after upgrading

A database whose sources already have symbol rows (built by the release before this one) is upgraded conservatively. The migration cannot prove that those rows still match each file's content, because an older writer may have changed a file without updating its symbols. It therefore adds the new `symbols_hash` column empty and does not trust the existing rows. The rows stay in the database, but `bpx-rag symbol` and `bpx_symbol` return no matches from a source until that source syncs. They report `Symbol index pending for source '<label>': N file(s) not yet symbol-indexed` instead, and `bpx-rag status` reports the same count.

Run one `bpx-rag sync --label <label>` per source after the upgrade (or `bpx-rag sync` for all). Sync rebuilds each file's symbols from the content it reads. It does not re-chunk or re-embed unchanged files.

### Rolling back

1. Stop every process that uses the database: `serve`, `watch run`, and any CLI command.
2. Optionally keep a copy of the current `knowledge.db` to inspect later.
3. Restore the backup you took before the upgrade over `knowledge.db`, and delete `knowledge.db-wal` and `knowledge.db-shm` next to it. A write-ahead log left by the newer build must not be replayed against the restored file.
4. Start the previous binary.

Restoring the backup discards every index change made after it was taken. Run the previous binary's `sync` to bring the sources current. If you have no backup, the previous builds tested here (the release before this one, and an earlier build that predates the identifier index) still open and write a migrated database. The identifier triggers, the queue table, and the version 2 identifier rows stay in the file, and older builds do not use them. Because the triggers remain, rows an older build writes are queued and indexed the next time a current build opens or syncs the database.

## Build from source

```bash
cargo build --release
```

## Data and privacy

bpx-rag runs locally and has no telemetry. The only network access is an explicit model download, either `models download` or the `bpx_models` download action, which fetches the embedding model from Hugging Face. Search, sync, and the MCP server otherwise make no network calls.

What is stored, and where:

- `knowledge.db` holds the indexed chunks, file hashes, source paths, and links. It lives in the local application data directory (`~/.local/share/bpx-rag` on Linux). The directory is created owner-only (0700), and the database file is restricted to its owner (0600) on Unix.
- Model files live in `models/` under the same directory. They are public weights and contain none of your data.
- Source configuration lives in `~/.config/bpx-rag/config.toml`. It holds settings only and no content.
- Nothing is written into the indexed projects.

What is indexed:

- The source walker skips credential directories and files, key and certificate containers, Terraform state, and high-confidence API-key signatures, even when they are not in `.gitignore`.
- `--include` and `--exclude` narrow what gets indexed. Use them to keep out generated, vendored, or personal folders.
- `scrub` removes rows that already match the policy. It reports counts only.

What is returned:

- Search results and `bpx_read` return snippets of indexed files to whichever client called the tool. Treat MCP clients as having read access to every source they can query.

Limits, reporting, and what is excluded are in [SECURITY.md](SECURITY.md).

Migrating from the old `bpcontext` name: if only `~/.local/share/bpcontext` or `~/.config/bpcontext` exists, bpx-rag refuses to start with an empty store and prints the `mv` or `ln -s` command to move it.
