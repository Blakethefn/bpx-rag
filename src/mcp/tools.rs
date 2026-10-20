use serde_json::{json, Value};

/// Generate the MCP tools/list response with all bpx_ tool definitions
pub fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "bpx_execute",
                "description": "Run a shell command, index the output, and return a preview. Use this instead of Bash for commands producing large output.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Shell command to execute"
                        },
                        "label": {
                            "type": "string",
                            "description": "Label for indexing (defaults to the command)"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Timeout in milliseconds (default: 120000)"
                        }
                    },
                    "required": ["command"]
                }
            },
            {
                "name": "bpx_batch_execute",
                "description": "Run multiple commands and search in one call. Replaces multiple bpx_execute + bpx_search calls.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "commands": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": { "type": "string" },
                                    "command": { "type": "string" }
                                },
                                "required": ["label", "command"]
                            },
                            "description": "Commands to execute"
                        },
                        "queries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Search queries to run against indexed output"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Total timeout in milliseconds"
                        }
                    },
                    "required": ["commands", "queries"]
                }
            },
            {
                "name": "bpx_search",
                "description": "Fuzzy and semantic retrieval over indexed documents and code. For exact imports, wikilinks, modules, or direct file impact, use bpx_file_relationships; it scans current files on demand without the knowledge database.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "queries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Search queries"
                        },
                        "source": {
                            "type": "string",
                            "description": "Filter by source label"
                        },
                        "content_type": {
                            "type": "string",
                            "enum": ["code", "prose"],
                            "description": "Filter by content type"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Max results per query (default: 15)"
                        },
                        "snippet_bytes": {
                            "type": "integer",
                            "description": "Max bytes per snippet for top results (default: 2000)"
                        },
                        "include_knowledge": {
                            "type": "boolean",
                            "description": "Include results from persistent knowledge sources (default: true)"
                        },
                        "filter": {
                            "type": "string",
                            "description": "Metadata filter for knowledge results (e.g., 'type:task status:active')"
                        },
                        "profile": {
                            "type": "string",
                            "enum": ["code_review", "debug", "planning", "vault_navigation"],
                            "description": "Optional retrieval profile that tunes ranking, snippets, graph behavior, and query expansion."
                        },
                        "explain": {
                            "type": "boolean",
                            "description": "If true, include a fuller per-result ranking explanation."
                        },
                        "use_memory": {
                            "type": "boolean",
                            "description": "Reuse exact-query learned search memory when available (default: true for explicit profiles)."
                        },
                        "count_only": {
                            "type": "boolean",
                            "description": "If true, returns only chunk count and estimated token size — no content. Use before large queries to decide whether to inline or delegate to a subagent."
                        }
                    },
                    "required": ["queries"]
                }
            },
            {
                "name": "bpx_file_relationships",
                "description": "Analyze direct file relationships from the current filesystem. Finds outgoing Markdown wikilinks, same-crate Rust module/import references, and relative TypeScript/JavaScript imports, plus files that directly reference the selected file. Returns paths and line numbers. Does not require or query the persistent knowledge database.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to analyze. May be absolute or relative to root/current directory."
                        },
                        "root": {
                            "type": "string",
                            "description": "Optional project root to scan. Defaults to the nearest recognized project root."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "bpx_execute_file",
                "description": "Read a file, optionally process it, and index the results.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path to read and index"
                        },
                        "language": {
                            "type": "string",
                            "description": "Language for syntax-aware processing"
                        },
                        "code": {
                            "type": "string",
                            "description": "Optional processing script to pipe the file through"
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query. For large files, returns relevant chunks instead of head-only preview."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "bpx_read_chunks",
                "description": "Retrieve full indexed content by source label. Returns chunks in original order and does not truncate them.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "label": {
                            "type": "string",
                            "description": "Exact source label to read back from the index"
                        },
                        "query": {
                            "type": "string",
                            "description": "Optional query to filter to only matching chunks within that source"
                        },
                        "max_chunks": {
                            "type": "integer",
                            "description": "Maximum chunks to return (default: 10)"
                        }
                    },
                    "required": ["label"]
                }
            },
            {
                "name": "bpx_fetch_and_index",
                "description": "Fetch a URL, convert HTML to markdown, and index it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to fetch"
                        },
                        "label": {
                            "type": "string",
                            "description": "Label for indexing (defaults to URL)"
                        }
                    },
                    "required": ["url"]
                }
            },
            {
                "name": "bpx_index",
                "description": "Index raw text for later search.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "Text content to index"
                        },
                        "label": {
                            "type": "string",
                            "description": "Label for the indexed content"
                        },
                        "content_type": {
                            "type": "string",
                            "enum": ["code", "prose"],
                            "description": "Content type hint"
                        }
                    },
                    "required": ["content", "label"]
                }
            },
            {
                "name": "bpx_promote",
                "description": "Promote indexed content to an obsidian output note via taskvault.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query to find content to promote"
                        },
                        "name": {
                            "type": "string",
                            "description": "Name for the output note"
                        },
                        "project": {
                            "type": "string",
                            "description": "Obsidian project to associate with"
                        }
                    },
                    "required": ["query", "name", "project"]
                }
            },
            {
                "name": "bpx_index_dir",
                "description": "Index all source files in a directory. Walks the directory respecting .gitignore, skips binary files, and indexes everything into the content store for later bpx_search. Run this at the start of exploration tasks to seed the search index.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path to index (e.g. \"./src\", \"/absolute/path/to/project\")"
                        },
                        "glob": {
                            "type": "string",
                            "description": "Glob pattern to filter files (e.g. \"*.rs\", \"*.py\"). Matches at any depth. If omitted, all non-binary text files are indexed."
                        },
                        "label_prefix": {
                            "type": "string",
                            "description": "Prefix for all labels (e.g. \"myproject/\"). Defaults to empty string."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "bpx_stats",
                "description": "Show context savings metrics for this session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "bpx_sources",
                "description": "List all indexed sources with chunk counts and timestamps. Use this to discover labels for bpx_read_chunks or bpx_search --source.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "bpx_context_status",
                "description": "Show context budget usage, per-source token breakdown, and relevance scores.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "bpx_knowledge_add",
                "description": "Register a directory as a persistent knowledge source and run the initial sync. The knowledge index persists across sessions — files are indexed once and updated incrementally on changes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the directory to index"
                        },
                        "label": {
                            "type": "string",
                            "description": "Unique label for this source (e.g., 'vault', 'docs')"
                        },
                        "glob": {
                            "type": "string",
                            "description": "File pattern filter (e.g., '**/*.md'). Default: all text files"
                        },
                        "enrichments": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Metadata enrichments to enable: 'frontmatter', 'wikilinks', 'folder_tags'"
                        }
                    },
                    "required": ["path", "label"]
                }
            },
            {
                "name": "bpx_knowledge_sync",
                "description": "Re-sync knowledge sources. Only re-indexes changed files (incremental). Run after bulk changes like git pulls.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "label": {
                            "type": "string",
                            "description": "Sync a specific source. If omitted, sync all sources."
                        }
                    }
                }
            },
            {
                "name": "bpx_knowledge_status",
                "description": "Show registered knowledge sources, sync state, and index statistics.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "bpx_knowledge_remove",
                "description": "Unregister a knowledge source and delete all its indexed content.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "label": {
                            "type": "string",
                            "description": "Source label to remove"
                        }
                    },
                    "required": ["label"]
                }
            },
            {
                "name": "bpx_knowledge_links",
                "description": "Traverse the knowledge graph. Given a file path or search query, find related chunks via wikilink edges. Supports forward (outgoing), backward (incoming), or both directions. Use count_only for graph mapping without content. Use filter to narrow by metadata.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "Relative path within a knowledge source (e.g. '01-projects/bpcontext.md'). Mutually exclusive with 'query'."
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query to find a starting file. The top result's file is used as the traversal origin. Mutually exclusive with 'file'."
                        },
                        "source": {
                            "type": "string",
                            "description": "Knowledge source label to scope the lookup."
                        },
                        "direction": {
                            "type": "string",
                            "enum": ["forward", "backward", "both"],
                            "description": "Traversal direction. Default: both."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum chunks to return. Default: 20."
                        },
                        "filter": {
                            "type": "string",
                            "description": "Metadata filter for linked files (e.g. 'type:task status:active tag:output folder:01-projects'). Multiple predicates are ANDed."
                        },
                        "count_only": {
                            "type": "boolean",
                            "description": "If true, returns link counts per file without chunk content. Useful for graph mapping and size estimation."
                        },
                        "depth": {
                            "type": "integer",
                            "description": "Traversal depth. 1 = direct links (default), 2 = links of links, max 3. Results include a hop annotation."
                        }
                    }
                }
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_relationship_tool_is_listed_with_required_path_and_optional_root() {
        let definitions = tool_definitions();
        let tools = definitions["tools"].as_array().unwrap();
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == "bpx_file_relationships")
            .expect("file relationship tool should be exposed over MCP");

        assert_eq!(tool["inputSchema"]["required"][0], "path");
        assert!(tool["inputSchema"]["properties"]["root"].is_object());
        assert!(tool["description"]
            .as_str()
            .unwrap()
            .contains("query the persistent knowledge database"));
    }
}
