use serde_json::json;

use super::super::handlers::{self, ModelAction, WatchAction};
use super::{Invoke, Tool};

pub(super) fn models() -> Tool {
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

pub(super) fn embed() -> Tool {
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

pub(super) fn symbol() -> Tool {
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

pub(super) fn findings() -> Tool {
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

pub(super) fn watch() -> Tool {
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

pub(super) fn relate() -> Tool {
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

