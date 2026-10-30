# Security and privacy

## Reporting a vulnerability

Report suspected vulnerabilities through GitHub's private vulnerability reporting on this repository. Do not open a public issue for them. Include the bpx-rag version, the command or tool call involved, and the steps to reproduce. Do not include real credentials or private source in the report.

## What bpx-rag stores

All state lives on the local machine:

- `knowledge.db` in the local application data directory (`~/.local/share/bpx-rag` on Linux). It holds indexed chunks, file hashes, source paths, and relation edges. The directory is created owner-only (mode 0700) and the database file is restricted to its owner (mode 0600) on Unix.
- Embedding model files in `models/` under the same directory. These are public weights.
- Settings in `~/.config/bpx-rag/config.toml`. These contain no indexed content.

bpx-rag writes nothing into the projects it indexes.

## Network access

bpx-rag makes no network calls during indexing, search, relation analysis, or the MCP server. The only network access is an explicit model download, through `bpx-rag models download` or the `bpx_models` MCP action.

## What is excluded from the index

The sync walker skips, even when they are not in `.gitignore`:

- version-control metadata directories
- credential-store directories and common credential files
- `.env` files, except names ending in `.example`, `.sample`, `.template`, `.dist`, or `.defaults`
- key and certificate containers
- Terraform state
- files whose content matches a high-confidence Google API-key signature

`bpx-rag scrub` removes rows that already match these rules. Its output reports counts only, never paths or contents.

## Limits

- The exclusions are name and pattern rules. They are not a secret scanner. Credentials with other formats, or stored in ordinary files, can still be indexed. Use `--exclude` to keep whole folders out.
- Anything in scope is indexed. Use `--include` and `--exclude` to narrow a source to what should be searchable.
- Search results and `bpx_read` return snippets of indexed files to the MCP client that asked. Treat any MCP client that can query a source as having read access to that source.
- The database is not encrypted. Protect the local user account and the disk.
- The MCP server runs with the permissions of the user that started it and does not authenticate clients. It is intended for local stdio use by the agent that launched it.
