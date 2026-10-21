use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufReader};
use std::path::Path;

use super::{tools, transport};

pub fn run(project_dir: &Path) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout().lock();

    while let Some(message) = transport::read_message(&mut reader)? {
        let request: Value = serde_json::from_str(&message)?;
        if let Some(response) = dispatch(&request, project_dir) {
            transport::write_message(&mut stdout, &response.to_string())?;
        }
    }
    Ok(())
}

fn dispatch(request: &Value, project_dir: &Path) -> Option<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let id = request.get("id").cloned();

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "bpx-rag",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "tools": {} }
            }
        })),
        "notifications/initialized" => None,
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": tools::tool_definitions()
        })),
        "tools/call" => Some(dispatch_tool(request, project_dir, id)),
        _ if id.is_some() => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        })),
        _ => None,
    }
}

fn dispatch_tool(request: &Value, project_dir: &Path, id: Option<Value>) -> Value {
    let name = request["params"]["name"].as_str().unwrap_or("");
    let args = &request["params"]["arguments"];
    let result = match name {
        "bpx_file_relationships" => handle_file_relationships(args, project_dir),
        _ => Err(anyhow::anyhow!("Unknown tool: {name}")),
    };

    match result {
        Ok(text) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": text }]
            }
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

fn handle_file_relationships(args: &Value, project_dir: &Path) -> Result<String> {
    let path = args["path"]
        .as_str()
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("path is required"))?;
    let root = args["root"].as_str().map(Path::new);
    let requested_path = Path::new(path);
    let resolved_path = if requested_path.is_absolute() || root.is_some() {
        requested_path.to_path_buf()
    } else {
        project_dir.join(requested_path)
    };
    let analysis = crate::relationships::analyze_file(&resolved_path, root)?;
    Ok(crate::relationships::format_analysis(&analysis))
}
