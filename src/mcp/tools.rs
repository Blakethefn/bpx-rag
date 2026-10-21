use serde_json::{json, Value};

pub fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "bpx_search",
                "description": "Hybrid retrieval across registered project sources. Returns ranked source chunks with file and line citations. Retrieval evidence only; use it to ground your answer in the connected language model.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "One natural-language or code search query." },
                        "queries": { "type": "array", "items": { "type": "string" }, "description": "Optional batch of search queries." },
                        "source": { "type": "string", "description": "Restrict results to a registered source label." },
                        "filter": { "type": "string", "description": "Metadata filter, e.g. `language:rust type:code`." },
                        "limit": { "type": "integer", "description": "Maximum results per query (1–50, default 10)." },
                        "profile": { "type": "string", "enum": ["code_review", "debug", "planning", "vault_navigation"] },
                        "explain": { "type": "boolean", "description": "Include ranking evidence for each result." },
                        "count_only": { "type": "boolean", "description": "Return result count and token estimate without chunk text." },
                        "use_memory": { "type": "boolean", "description": "Allow a prior explicit-profile search preference to influence this query." }
                    },
                    "required": []
                }
            },
            {
                "name": "bpx_knowledge_add",
                "description": "Register and initially index a local project or documentation directory in the persistent RAG store.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory to index." },
                        "label": { "type": "string", "description": "Unique source label." },
                        "glob": { "type": "string", "description": "Optional relative file glob." },
                        "enrichments": { "type": "array", "items": { "type": "string" }, "description": "Optional metadata enrichments such as frontmatter and wikilinks." }
                    },
                    "required": ["path", "label"]
                }
            },
            {
                "name": "bpx_knowledge_sync",
                "description": "Incrementally update registered sources, replacing changed files and removing files deleted since the last successful sync.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "label": { "type": "string", "description": "Optional source label; omit to sync all sources." } },
                    "required": []
                }
            },
            {
                "name": "bpx_knowledge_status",
                "description": "List registered sources, sync state, chunk totals, and embedding coverage.",
                "inputSchema": { "type": "object", "properties": {}, "required": [] }
            },
            {
                "name": "bpx_knowledge_remove",
                "description": "Remove a registered source and all of its indexed data.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "label": { "type": "string" } },
                    "required": ["label"]
                }
            },
            {
                "name": "bpx_knowledge_read",
                "description": "Read full indexed chunks from a source. Use the cited relative path and line range from bpx_search to expand the specific hit.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "max_chunks": { "type": "integer", "description": "Maximum number of matching chunks (1–100, default 20)." },
                        "path": { "type": "string", "description": "Optional source-relative path from a search result." },
                        "line_start": { "type": "integer", "description": "Optional 1-based first line of the cited range." },
                        "line_end": { "type": "integer", "description": "Optional 1-based last line of the cited range." }
                    },
                    "required": ["label"]
                }
            },
            {
                "name": "bpx_embeddings",
                "description": "Inspect or explicitly download the local embedding model, or backfill vectors for indexed chunks. Backfill can be restricted to one registered source label.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["status", "download", "backfill"] },
                        "label": { "type": "string", "description": "Optional exact knowledge source label for backfill; omitted means all registered sources." }
                    },
                    "required": ["action"]
                }
            },
            {
                "name": "bpx_file_relationships",
                "description": "Analyze direct imports, includes, and links for one current file. Returns exact paths and line evidence without querying the RAG index.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path to analyze." },
                        "root": { "type": "string", "description": "Optional project root; otherwise inferred from project markers." }
                    },
                    "required": ["path"]
                }
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_rag_and_direct_relationship_tools() {
        let definitions = tool_definitions();
        let names = definitions["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"bpx_search"));
        assert!(names.contains(&"bpx_knowledge_add"));
        assert!(names.contains(&"bpx_knowledge_sync"));
        assert!(names.contains(&"bpx_knowledge_status"));
        assert!(names.contains(&"bpx_knowledge_read"));
        assert!(names.contains(&"bpx_embeddings"));
        assert!(names.contains(&"bpx_file_relationships"));
    }
}
