use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufReader};
use std::path::Path;

use super::{tools, transport};
use crate::app::RagApp;

pub fn run(project_dir: &Path) -> Result<()> {
    start_watcher();
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

fn start_watcher() {
    let db_path = match crate::db::data_dir() {
        Ok(dir) => dir.join("knowledge.db"),
        Err(error) => {
            eprintln!("[bpx-rag] watch: could not locate the database: {error:#}");
            return;
        }
    };
    if !db_path.is_file() {
        return;
    }
    std::thread::spawn(move || {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let outcome = crate::watch::run_with_loader(
            &db_path,
            &crate::watch::WatchOptions::default(),
            &stop,
            Box::new(crate::app::load_watch_embedder),
        );
        if let Err(error) = outcome {
            eprintln!("[bpx-rag] watch stopped: {error:#}");
        }
    });
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
    let result = tools::call_tool(name, args, project_dir, app_slot);

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

#[cfg(test)]
mod tests;
