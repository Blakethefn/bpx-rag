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
- C/C++ quoted includes, C# namespace imports, Go imports, Java/Kotlin imports, Python imports, PHP includes/requires, Ruby requires, Bash `source`, Lua `require`, Dart imports/exports, and Swift module imports. Most use statically linked Tree-sitter grammars; Kotlin uses the existing comment-aware lexer.
- Direct incoming references as well as outgoing references from the selected file.

The added language grammars are linked into the binary; analysis does not download parsers at runtime. Results cover direct file relationships. Package imports are linked only when they map conservatively to files under the project root. TypeScript `tsconfig` aliases, Cargo cross-crate aliases, compiler-specific module maps, symbol-level calls, and transitive runtime effects are not resolved.

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
