use serde_json::json;
use tempfile::tempdir;

use super::client::{call, tool_text};

#[test]
fn bpx_findings_rejects_a_missing_label() {
    let temp = tempdir().unwrap();
    let complete = temp.path().join("complete");
    let broken = temp.path().join("broken");
    std::fs::create_dir_all(&complete).unwrap();
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    std::fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

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

    let added_broken = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "bpx_add",
                "arguments": { "path": broken, "label": "partial" }
            }
        })],
    );
    assert!(
        !added_broken[0]["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        "{}",
        tool_text(&added_broken[0])
    );

    let missing = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "bpx_findings", "arguments": {} }
        })],
    );
    let missing_text = tool_text(&missing[0]);
    assert_eq!(missing[0]["result"]["isError"], true, "{missing_text}");
    assert!(missing_text.contains("ready"), "{missing_text}");
    assert!(missing_text.contains("partial"), "{missing_text}");
}
