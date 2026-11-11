use super::{indexed_fixture, Fixture};

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn mcp_responses(fixture: &Fixture, calls: &[Value]) -> Vec<Value> {
    let mut child = Command::new(std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap())
        .arg("serve")
        .current_dir(&fixture.project)
        .env("XDG_CONFIG_HOME", &fixture.config_home)
        .env("XDG_DATA_HOME", &fixture.data_home)
        .env("XDG_CACHE_HOME", &fixture.cache_home)
        .env_remove("HF_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}})
        )
        .unwrap();
        for call in calls {
            writeln!(stdin, "{call}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn search_call(id: u64, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": "bpx_search", "arguments": arguments }
    })
}

fn response(responses: &[Value], id: u64) -> &Value {
    responses
        .iter()
        .find(|response| response["id"] == id)
        .unwrap_or_else(|| panic!("no response for id {id}: {responses:?}"))
}

fn response_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

#[test]
fn mcp_search_accepts_max_per_file_and_rejects_out_of_range_values() {
    let fixture = indexed_fixture();
    let base = |extra: Value| {
        let mut arguments = json!({"query": "kestrel", "source": "docs", "limit": 3});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        arguments
    };
    let responses = mcp_responses(
        &fixture,
        &[
            search_call(1, base(json!({"max_per_file": 3}))),
            search_call(2, base(json!({"max_per_file": 0}))),
            search_call(3, base(json!({"max_per_file": 51}))),
            search_call(4, base(json!({}))),
        ],
    );

    let capped = response(&responses, 1);
    assert_ne!(capped["result"]["isError"], json!(true), "{capped}");
    let capped_text = response_text(capped);
    assert_eq!(
        capped_text.matches("file: big.md").count(),
        3,
        "{capped_text}"
    );
    assert!(!capped_text.contains("file: small.md"), "{capped_text}");

    for id in [2, 3] {
        let rejected = response(&responses, id);
        assert_eq!(rejected["result"]["isError"], json!(true), "{rejected}");
        assert!(
            response_text(rejected).contains("max_per_file"),
            "{rejected}"
        );
    }

    let default = response(&responses, 4);
    assert!(
        response_text(default).contains("file: small.md"),
        "{default}"
    );
}
