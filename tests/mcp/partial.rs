use serde_json::json;
use tempfile::tempdir;

use super::client::{call, tool_text};

#[test]
fn bpx_findings_returns_incomplete_analysis_rows() {
    let temp = tempdir().unwrap();
    let broken = temp.path().join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

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

    let incomplete = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "bpx_findings",
                "arguments": { "source": "partial" }
            }
        })],
    );
    let incomplete_text = tool_text(&incomplete[0]);
    assert!(
        !incomplete[0]["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        "{incomplete_text}"
    );
    assert!(
        incomplete_text.contains("status: incomplete"),
        "{incomplete_text}"
    );
    assert!(incomplete_text.contains("broken.rs"), "{incomplete_text}");
    assert!(
        incomplete_text.contains("fingerprint:"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("possibly_unused_symbol"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("unreachable_file"),
        "{incomplete_text}"
    );
}
