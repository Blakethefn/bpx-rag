use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub(crate) fn call(data: &Path, requests: &[Value]) -> Vec<Value> {
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

pub(crate) fn tool_text(response: &Value) -> String {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}
