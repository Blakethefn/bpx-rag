use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn call(data: &std::path::Path, requests: &[Value]) -> Vec<Value> {
    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap();
    let mut child = Command::new(executable)
        .arg("serve")
        .env("XDG_CONFIG_HOME", data.join("config"))
        .env("XDG_DATA_HOME", data.join("data"))
        .env("XDG_CACHE_HOME", data.join("cache"))
        .env_remove("HF_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

fn tool_text(response: &Value) -> String {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn bpx_findings_returns_analysis_rows_and_rejects_a_missing_label() {
    let temp = tempdir().unwrap();
    let complete = temp.path().join("complete");
    let broken = temp.path().join("broken");
    std::fs::create_dir_all(&complete).unwrap();
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    std::fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

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
