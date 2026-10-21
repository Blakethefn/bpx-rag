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

Sources are persisted in a local SQLite knowledge database and incrementally synced using content hashes and per-file chunker versions. RAG chunking has bundled Tree-sitter grammars for 29 source languages: Rust, JavaScript/TypeScript/TSX, C/C++, C#, Go, Java, Python, PHP, Ruby, Bash, Lua, Dart, Swift, Kotlin, SQL, Scala, Haskell, Elixir, Erlang, R, Julia, Perl, PowerShell, Solidity, HTML, and CSS. Tree-sitter declaration boundaries keep functions and related syntax together; Markdown, unsupported extensions, parse-error files, and source files over 2 MiB use the existing content chunker. Chunks remain capped at 4 KiB. When a supported code file is first encountered after upgrading, its stored legacy chunker version causes it to be reindexed on the next sync even if its content is unchanged. Sync summaries report Tree-sitter and existing-chunker file/chunk counts for files indexed during that operation. Search works immediately with BM25/trigram/fuzzy retrieval. Semantic search becomes available after an embedding model has been downloaded and indexed. The source walker skips VCS metadata directories (`.git`, `.hg`, `.svn`), common credential-store directories (`.aws`, `.azure`, `.gnupg`, `.kube`, `.ssh`, `.terraform`, `credentials`, `secrets`), common credential files, `.env` files other than names ending in `.example`, `.sample`, `.template`, `.dist`, or `.defaults`, key/certificate containers (`.key`, `.pem`, `.p12`, `.pfx`, `.jks`, `.keystore`), and Terraform state—even when those paths are not in `.gitignore`. It also skips files matching a high-confidence Google API-key signature. `knowledge scrub-sensitive` removes already-indexed rows matched by the path/name policy or the recognized key signature, without walking or indexing source trees; it reports aggregate counts only. Sync and scrub summaries never list skipped paths or contents. Sample-file exceptions are based on the filename suffix and still require placeholder contents; benign `.npmrc`, `.pypirc`, or PEM certificate files are also excluded by this conservative rule. These are targeted safeguards, not comprehensive secret scanning; other secret formats may still be indexed.

## Embedding models

Model downloads are explicit:

```bash
bpx-rag models status
bpx-rag models download
bpx-rag knowledge embed-backfill
# Backfill one registered source at a time for large knowledge stores.
bpx-rag knowledge embed-backfill --label my-project
```

The default `jina-embeddings-v2-base-code` model is tuned for code retrieval across 30 programming languages and supports an 8K token input window ([upstream model card](https://huggingface.co/jinaai/jina-embeddings-v2-base-code)). bpx-rag caps each embedded chunk at 1,024 tokens to control local inference cost; the full local model artifacts take about 644 MB. Set `embeddings.model` in `~/.config/bpcontext/config.toml` to `all-MiniLM-L6-v2` to use the smaller 91 MB general-purpose model.

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

CPU is the default. To enable CUDA, build with `--features gpu-cuda`, set `embeddings.device = "cuda"`, and make the CUDA and cuDNN runtime libraries discoverable by the process. The CUDA execution provider is configured to fail clearly when it cannot register.

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

RAG tools include `bpx_search`, `bpx_knowledge_add`, `bpx_knowledge_sync`, `bpx_knowledge_status`, `bpx_knowledge_remove`, `bpx_knowledge_read`, and `bpx_embeddings`. `bpx_knowledge_read` accepts the cited source-relative path and line range. `bpx_file_relationships` analyzes the current filesystem and does not need the RAG index.

## Direct file relationships

```bash
bpx-rag analyze-file src/lib/api/quotes.ts --root /path/to/project
```

The analyzer reports direct Markdown wikilinks, imports, includes, and module references with line evidence for Rust, TypeScript/JavaScript, C/C++, C#, Go, Java, Kotlin, Scala, Haskell, Erlang, Elixir, R, Julia, Perl, PowerShell, Solidity, Python, PHP, Ruby, Bash, Lua, Dart, Swift, HTML, and CSS. SQL has syntax-aware RAG chunking but no file-import extraction. Static analysis resolves local paths conservatively; it does not infer runtime behavior, symbol-level calls, or build-specific aliases. External packages and web URLs are not treated as local files.

Syntax-aware Tree-sitter chunking supports 29 source grammars. Parse-error and oversized files retain the existing chunker.

## Build from source

```bash
cargo build --release
```

`bpcontext` remains as a compatibility launcher for older installations; new integrations should run `bpx-rag serve`.
