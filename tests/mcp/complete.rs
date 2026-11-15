use serde_json::json;
use tempfile::tempdir;

use super::client::{call, tool_text};

#[test]
fn bpx_findings_returns_complete_analysis_rows() {
    let temp = tempdir().unwrap();
    let complete = temp.path().join("complete");
    std::fs::create_dir_all(&complete).unwrap();
    std::fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();

    let listed = call(
        temp.path(),
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        ],
    );
    let names = listed[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"bpx_findings"), "{names:?}");

    let added = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "bpx_add",
                "arguments": { "path": complete, "label": "ready" }
            }
        })],
    );
    assert!(
        !added[0]["result"]["isError"].as_bool().unwrap_or(false),
        "{}",
        tool_text(&added[0])
    );

    let one = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "bpx_findings", "arguments": {} }
        })],
    );
    let one_text = tool_text(&one[0]);
    assert!(
        !one[0]["result"]["isError"].as_bool().unwrap_or(false),
        "{one_text}"
    );
    assert!(one_text.contains("source: ready"), "{one_text}");
    assert!(one_text.contains("status: complete"), "{one_text}");
    assert!(one_text.contains("fingerprint:"), "{one_text}");
    assert!(one_text.contains("findings:"), "{one_text}");
}
