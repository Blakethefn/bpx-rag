use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

use super::handlers::{self, ModelAction, WatchAction};
use crate::app::RagApp;
use crate::search::profile::SearchProfile;

pub fn tool_definitions() -> Value {
    json!({
        "tools": catalog().iter().map(Tool::definition).collect::<Vec<_>>()
    })
}

pub(super) fn call_tool(
    name: &str,
    args: &Value,
    project_dir: &Path,
    app_slot: &mut Option<RagApp>,
) -> Result<String> {
    tool_by_name(name)
        .ok_or_else(|| anyhow!("unknown tool: {name}"))?
        .call(args, project_dir, app_slot)
}

struct Tool {
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &'static [&'static str],
    invoke: Invoke,
}

enum Invoke {
    App(fn(&Value, &mut RagApp) -> Result<String>),
    Relate,
}

impl Tool {
    fn definition(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": {
                "type": "object",
                "properties": self.properties.clone(),
                "required": self.required,
            }
        })
    }

    fn call(
        self,
        args: &Value,
        project_dir: &Path,
        app_slot: &mut Option<RagApp>,
    ) -> Result<String> {
        match self.invoke {
            Invoke::App(handler) => with_app(app_slot, |app| handler(args, app)),
            Invoke::Relate => handlers::handle_file_relationships(args, project_dir),
        }
    }
}

fn with_app(
    slot: &mut Option<RagApp>,
    operation: impl FnOnce(&mut RagApp) -> Result<String>,
) -> Result<String> {
    if slot.is_none() {
        *slot = Some(RagApp::open()?);
    }
    operation(slot.as_mut().expect("RAG app was just initialized"))
}

fn tool_by_name(name: &str) -> Option<Tool> {
    catalog().into_iter().find(|tool| tool.name == name)
}

fn catalog() -> Vec<Tool> {
    vec![
        search(),
        add(),
        sync(),
        status(),
        remove(),
        read(),
        models(),
        embed(),
        symbol(),
        findings(),
        watch(),
        relate(),
    ]
}

fn search() -> Tool {
    Tool {
        name: "bpx_search",
        description: "Hybrid retrieval across registered project sources. Returns ranked source chunks with file and line citations. Retrieval evidence only; use it to ground your answer in the connected language model.",
        properties: json!({
            "query": { "type": "string", "description": "One natural-language or code search query." },
            "queries": { "type": "array", "items": { "type": "string" }, "description": "Optional batch of search queries." },
            "source": { "type": "string", "description": "Restrict results to a registered source label." },
            "filter": { "type": "string", "description": "Metadata filter, e.g. `language:rust type:code`." },
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

fn add() -> Tool {
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

fn sync() -> Tool {
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

fn status() -> Tool {
    Tool {
        name: "bpx_status",
        description: "List registered sources, sync state, chunk totals, and embedding coverage.",
        properties: json!({}),
        required: &[],
        invoke: Invoke::App(|_args, app| app.source_status()),
    }
}

fn remove() -> Tool {
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

fn read() -> Tool {
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

fn models() -> Tool {
    Tool {
        name: "bpx_models",
        description: "Inspect or explicitly download the local embedding model.",
        properties: json!({
            "action": { "type": "string", "enum": ModelAction::wire_names() }
        }),
        required: &["action"],
        invoke: Invoke::App(handlers::handle_models),
    }
}

fn embed() -> Tool {
    Tool {
        name: "bpx_embed",
        description: "Compute missing vectors for indexed chunks with the configured model. Can be restricted to one registered source label.",
        properties: json!({
            "label": { "type": "string", "description": "Optional exact source label; omitted means all registered sources." }
        }),
        required: &[],
        invoke: Invoke::App(handlers::handle_embed),
    }
}

fn symbol() -> Tool {
    Tool {
        name: "bpx_symbol",
        description: "Find declarations (functions, types, methods, modules, constants) by name across registered sources. Returns exact paths and line ranges for every match. Exact match by default; set prefix for names that start with the given text. Matching is case-sensitive.",
        properties: json!({
            "name": { "type": "string", "description": "Declaration name to find." },
            "prefix": { "type": "boolean", "description": "Match names that start with `name` instead of matching it exactly." },
            "source": { "type": "string", "description": "Restrict results to a registered source label." },
            "limit": { "type": "integer", "description": "Maximum matches (1–100, default 20)." }
        }),
        required: &["name"],
        invoke: Invoke::App(handlers::handle_symbol),
    }
}

fn findings() -> Tool {
    Tool {
        name: "bpx_findings",
        description: "Read stored unused-symbol and unreachable-file conclusions for one source. Returns the analysis row first. Finding rows are included only when the analysis is complete and each row's version, generation, and fingerprint match it. Pass source when more than one source is registered.",
        properties: json!({
            "source": { "type": "string", "description": "Registered source label. Omit only when exactly one source is registered." }
        }),
        required: &[],
        invoke: Invoke::App(handlers::handle_findings),
    }
}

fn watch() -> Tool {
    Tool {
        name: "bpx_watch",
        description: "Control watch mode, which keeps sources synchronized as files change. action `on` or `off` sets it for one source (label required). action `status` reports the watcher and each source's state. A watcher runs inside one MCP server or `bpx-rag watch run` at a time.",
        properties: json!({
            "action": { "type": "string", "enum": WatchAction::wire_names() },
            "label": { "type": "string", "description": "Source label, for `on` and `off`." }
        }),
        required: &["action"],
        invoke: Invoke::App(handlers::handle_watch),
    }
}

fn relate() -> Tool {
    Tool {
        name: "bpx_relate",
        description: "Analyze direct imports, includes, and links for one current file. Returns exact paths and line evidence without querying the RAG index.",
        properties: json!({
            "path": { "type": "string", "description": "File path to analyze." },
            "root": { "type": "string", "description": "Optional project root; otherwise inferred from project markers." }
        }),
        required: &["path"],
        invoke: Invoke::Relate,
    }
}

#[cfg(test)]
mod tests;
