use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::io::{self, BufReader};
use std::path::Path;

use super::{tools, transport};
use crate::app::{RagApp, SearchOptions};

pub fn run(project_dir: &Path) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout().lock();
    let mut app: Option<RagApp> = None;

    while let Some(message) = transport::read_message(&mut reader)? {
        let request: Value = serde_json::from_str(&message)?;
        if let Some(response) = dispatch(&request, project_dir, &mut app) {
            transport::write_message(&mut stdout, &response.to_string())?;
        }
    }
    Ok(())
}

fn dispatch(request: &Value, project_dir: &Path, app: &mut Option<RagApp>) -> Option<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let id = request.get("id").cloned();
    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "bpx-rag", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "tools": {} }
            }
        })),
        "notifications/initialized" => None,
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": tools::tool_definitions()
        })),
        "tools/call" => Some(dispatch_tool(request, project_dir, app, id)),
        _ if id.is_some() => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        })),
        _ => None,
    }
}

fn dispatch_tool(
    request: &Value,
    project_dir: &Path,
    app_slot: &mut Option<RagApp>,
    id: Option<Value>,
) -> Value {
    let name = request["params"]["name"].as_str().unwrap_or("");
    let args = &request["params"]["arguments"];
    let result = match name {
        "bpx_relate" => handle_file_relationships(args, project_dir),
        "bpx_symbol" => with_app(app_slot, |app| handle_symbol(args, app)),
        "bpx_search" => with_app(app_slot, |app| handle_search(args, app)),
        "bpx_add" => with_app(app_slot, |app| handle_knowledge_add(args, app)),
        "bpx_sync" => with_app(app_slot, |app| handle_knowledge_sync(args, app)),
        "bpx_status" => with_app(app_slot, |app| app.source_status()),
        "bpx_remove" => with_app(app_slot, |app| handle_knowledge_remove(args, app)),
        "bpx_read" => with_app(app_slot, |app| handle_knowledge_read(args, app)),
        "bpx_models" => with_app(app_slot, |app| handle_models(args, app)),
        "bpx_embed" => with_app(app_slot, |app| handle_embed(args, app)),
        _ => Err(anyhow!("unknown tool: {name}")),
    };

    match result {
        Ok(text) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "content": [{ "type": "text", "text": text }] }
        }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": format!("Error: {error}") }],
                "isError": true
            }
        }),
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

fn handle_search(args: &Value, app: &mut RagApp) -> Result<String> {
    let queries = if let Some(queries) = args["queries"].as_array() {
        queries
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        args["query"]
            .as_str()
            .map(str::to_string)
            .into_iter()
            .collect()
    };
    if queries.is_empty() || queries.len() > 8 {
        return Err(anyhow!("provide `query` or 1–8 non-empty `queries`"));
    }
    let options = SearchOptions {
        source: args["source"].as_str(),
        filter: args["filter"].as_str(),
        limit: bounded_u32(args, "limit", 10, 1, 50)?,
        profile: args["profile"].as_str(),
        explain: args["explain"].as_bool().unwrap_or(false),
        count_only: args["count_only"].as_bool().unwrap_or(false),
    };
    let mut output = String::new();
    let mut count_results = Vec::new();
    for query in queries {
        if query.trim().is_empty() {
            return Err(anyhow!("queries must not be empty"));
        }
        let result = app.search(&query, options.clone())?;
        if options.count_only {
            count_results.push(serde_json::from_str::<Value>(&result)?);
            continue;
        }
        if queries_len_multi(&args["queries"]) {
            output.push_str(&format!("# Query: {}\n\n", query.trim()));
        }
        output.push_str(&result);
        output.push_str("\n\n");
    }
    if options.count_only && count_results.len() > 1 {
        return Ok(serde_json::to_string_pretty(
            &json!({ "queries": count_results }),
        )?);
    }
    if options.count_only {
        return Ok(serde_json::to_string_pretty(&count_results[0])?);
    }
    Ok(output.trim_end().to_string())
}

fn handle_knowledge_add(args: &Value, app: &mut RagApp) -> Result<String> {
    let path = required_string(args, "path")?;
    let label = required_string(args, "label")?;
    let enrichments = string_list(args, "enrichments")?;
    let mut include = string_list(args, "include")?;
    if let Some(glob) = args["glob"].as_str() {
        include.insert(0, glob.to_string());
    }
    let exclude = string_list(args, "exclude")?;
    app.add_source(Path::new(path), label, &include, &exclude, &enrichments)
}

/// Read an optional array of strings. Any other shape is an error, not an empty list.
fn string_list(args: &Value, key: &str) -> Result<Vec<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("'{key}' must contain only strings"))
            })
            .collect(),
        Some(_) => Err(anyhow!("'{key}' must be an array of strings")),
    }
}

fn handle_knowledge_sync(args: &Value, app: &mut RagApp) -> Result<String> {
    let results = app.sync_sources(args["label"].as_str())?;
    if results.is_empty() {
        return Ok("No knowledge sources are registered.".into());
    }
    Ok(results
        .iter()
        .map(crate::app::format_sync_result)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn handle_knowledge_remove(args: &Value, app: &RagApp) -> Result<String> {
    let label = required_string(args, "label")?;
    let result = app.remove_source(label)?;
    Ok(format!(
        "Removed source '{label}' ({} files, {} chunks).",
        result.files_removed, result.chunks_removed
    ))
}

fn handle_knowledge_read(args: &Value, app: &RagApp) -> Result<String> {
    let label = required_string(args, "label")?;
    app.read_chunks(
        label,
        bounded_usize(args, "max_chunks", 20, 1, 100)?,
        args["path"].as_str(),
        optional_bounded_u32(args, "line_start")?,
        optional_bounded_u32(args, "line_end")?,
    )
}

fn handle_models(args: &Value, app: &mut RagApp) -> Result<String> {
    match args["action"].as_str().unwrap_or("") {
        "status" => Ok(app.embedding_model_status()),
        "download" => app.download_embedding_model(),
        action => Err(anyhow!("unknown models action '{action}'")),
    }
}

fn handle_embed(args: &Value, app: &mut RagApp) -> Result<String> {
    let label = match args.get("label") {
        None | Some(Value::Null) => None,
        Some(Value::String(label)) => Some(label.as_str()),
        Some(_) => return Err(anyhow!("'label' must be a string")),
    };
    Ok(app.backfill_embeddings(label)?.summary())
}

fn handle_symbol(args: &Value, app: &mut RagApp) -> Result<String> {
    let name = required_string(args, "name")?;
    let mode = if args["prefix"].as_bool().unwrap_or(false) {
        crate::knowledge::symbols::MatchMode::Prefix
    } else {
        crate::knowledge::symbols::MatchMode::Exact
    };
    let limit = bounded_usize(args, "limit", 20, 1, crate::app::MAX_SYMBOL_LIMIT)?;
    app.find_symbols(name, mode, args["source"].as_str(), limit)
}

fn handle_file_relationships(args: &Value, project_dir: &Path) -> Result<String> {
    let path = required_string(args, "path")?;
    let root = args["root"].as_str().map(Path::new);
    let requested = Path::new(path);
    let resolved = if requested.is_absolute() || root.is_some() {
        requested.to_path_buf()
    } else {
        project_dir.join(requested)
    };
    let analysis = crate::relations::analyze_file(&resolved, root)?;
    Ok(crate::relations::format_analysis(&analysis))
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("'{key}' is required"))
}

fn bounded_u32(args: &Value, key: &str, default: u32, min: u32, max: u32) -> Result<u32> {
    let value = match args[key].as_u64() {
        Some(value) => u32::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}

fn optional_bounded_u32(args: &Value, key: &str) -> Result<Option<u32>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or_else(|| anyhow!("'{key}' must be a positive integer"))?;
            if number == 0 || number > u32::MAX as u64 {
                return Err(anyhow!("'{key}' must be between 1 and {}", u32::MAX));
            }
            Ok(Some(number as u32))
        }
    }
}

fn bounded_usize(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = match args[key].as_u64() {
        Some(value) => usize::try_from(value).map_err(|_| anyhow!("'{key}' is out of range"))?,
        None => default,
    };
    if !(min..=max).contains(&value) {
        return Err(anyhow!("'{key}' must be between {min} and {max}"));
    }
    Ok(value)
}

fn queries_len_multi(value: &Value) -> bool {
    value.as_array().is_some_and(|queries| queries.len() > 1)
}
