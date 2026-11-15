use serde_json::json;

use super::super::handlers;
use super::{Invoke, Tool};
use crate::search::profile::SearchProfile;

pub(super) fn search() -> Tool {
    Tool {
        name: "bpx_search",
        description: "Hybrid retrieval across registered project sources. Returns ranked source chunks with file and line citations. Retrieval evidence only; use it to ground your answer in the connected language model.",
        properties: json!({
            "query": { "type": "string", "description": "One natural-language or code search query." },
            "queries": { "type": "array", "items": { "type": "string" }, "description": "Optional batch of search queries." },
            "source": { "type": "string", "description": "Restrict results to a registered source label." },
            "filter": { "type": "string", "description": "Metadata or indexed-path filter, e.g. `language:rust type:code`, `dir:src`, or `file:\"src/main.rs\"`. Quote path values that contain spaces." },
            "limit": { "type": "integer", "description": "Maximum results per query (1–50, default 10)." },
            "max_per_file": { "type": "integer", "description": "Preferred cap on passages returned from one file (1–50, default 1). Passages from one file are taken first, up to this number. If the other files cannot fill the results, the remaining slots take the next-best passages from the same file, so the cap is not strict." },
            "profile": { "type": "string", "enum": SearchProfile::input_names() },
            "explain": { "type": "boolean", "description": "Include ranking evidence for each result, what each retrieval layer did, and a warning first when vector retrieval is not running." },
            "count_only": { "type": "boolean", "description": "Return result count and token estimate without chunk text." }
        }),
        required: &[],
        invoke: Invoke::App(handlers::handle_search),
    }
}

pub(super) fn add() -> Tool {
    Tool {
        name: "bpx_add",
        description: "Register and initially index a local project or documentation directory in the persistent RAG store.",
        properties: json!({
            "path": { "type": "string", "description": "Directory to index." },
            "label": { "type": "string", "description": "Unique source label." },
            "include": { "type": "array", "items": { "type": "string" }, "description": "Optional glob patterns. When present, only matching paths are indexed." },
            "exclude": { "type": "array", "items": { "type": "string" }, "description": "Optional glob patterns to skip, such as `**/*.test.ts`." },
            "glob": { "type": "string", "description": "Deprecated alias for one `include` pattern." },
            "enrichments": { "type": "array", "items": { "type": "string" }, "description": "Optional metadata enrichments such as frontmatter and wikilinks." }
        }),
        required: &["path", "label"],
        invoke: Invoke::App(handlers::handle_knowledge_add),
    }
}

pub(super) fn sync() -> Tool {
    Tool {
        name: "bpx_sync",
        description: "Incrementally update registered sources, replacing changed files and removing files deleted since the last successful sync.",
        properties: json!({
            "label": { "type": "string", "description": "Optional source label; omit to sync all sources." }
        }),
        required: &[],
        invoke: Invoke::App(handlers::handle_knowledge_sync),
    }
}

pub(super) fn status() -> Tool {
    Tool {
        name: "bpx_status",
        description: "List registered sources, sync state, chunk totals, and embedding coverage.",
        properties: json!({}),
        required: &[],
        invoke: Invoke::App(|_args, app| app.source_status()),
    }
}

pub(super) fn remove() -> Tool {
    Tool {
        name: "bpx_remove",
        description: "Remove a registered source and all of its indexed data.",
        properties: json!({
            "label": { "type": "string" }
        }),
        required: &["label"],
        invoke: Invoke::App(handlers::handle_knowledge_remove),
    }
}

pub(super) fn read() -> Tool {
    Tool {
        name: "bpx_read",
        description: "Read full indexed chunks from a source. Use the cited relative path and line range from bpx_search to expand the specific hit.",
        properties: json!({
            "label": { "type": "string" },
            "max_chunks": { "type": "integer", "description": "Maximum number of matching chunks (1–100, default 20)." },
            "path": { "type": "string", "description": "Optional source-relative path from a search result." },
            "line_start": { "type": "integer", "description": "Optional 1-based first line of the cited range." },
            "line_end": { "type": "integer", "description": "Optional 1-based last line of the cited range." }
        }),
        required: &["label"],
        invoke: Invoke::App(handlers::handle_knowledge_read),
    }
}
