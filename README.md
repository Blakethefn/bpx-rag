# bpx-rag

**Fast, filesystem-first file relationship analysis for coding agents.**

Point bpx-rag at a file to see what it imports or links to, and which files directly refer back to it. It reads the current project files and returns paths with line evidence. It does not need a database, embeddings, config, or an indexing step.

## Quick start

```bash
# Direct imports and files that refer to this file
bpx-rag analyze-file src/lib/api/quotes.ts --root /path/to/project

# The project root can be inferred from a nearby .git, Cargo.toml,
# package.json, or pyproject.toml marker
bpx-rag analyze-file src/lib/api/quotes.ts
```

The selected file and project root must be inside the same project. Scanning respects ignore rules and skips generated directories. The analyzer has safety limits of 20,000 files, 128 MiB total scanned content, and 2 MiB per file; it warns if the scan is incomplete.

## Supported relationships

- Markdown wikilinks, including aliases and headings.
- Same-crate Rust `mod` and `use` paths.
- Relative TypeScript and JavaScript imports, re-exports, dynamic imports, and `require()` calls.
- Direct incoming references as well as outgoing references from the selected file.

Results cover direct file relationships. TypeScript `tsconfig` path aliases, Cargo cross-crate aliases, symbol-level calls, and transitive runtime effects are not resolved yet.

## MCP

Add bpx-rag to your MCP client:

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

The server exposes one tool, `bpx_file_relationships`:

```json
{
  "path": "/path/to/project/src/lib/api/quotes.ts",
  "root": "/path/to/project"
}
```

`root` is optional. MCP startup and relationship calls do not read config files, open databases, or load models.

## Build and test

```bash
cargo build --release
cargo test
```

The `bpcontext` executable remains as a compatibility launcher for older scripts. It forwards arguments to `bpx-rag`.
