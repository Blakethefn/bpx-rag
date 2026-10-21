use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn mcp_relationship_tool_scans_files_without_creating_runtime_state() {
    let project = tempdir().unwrap();
    let project_root = project.path().join("project");
    let config_dir = project_root.join("runtime-config");
    let data_dir = project_root.join("runtime-data");
    let cache_dir = project_root.join("runtime-cache");
    std::fs::create_dir_all(project_root.join("src/lib/api")).unwrap();
    std::fs::create_dir_all(project_root.join("src/hooks")).unwrap();
    std::fs::write(
        project_root.join("src/lib/api/quotes.ts"),
        "export function getQuotes() {}\n",
    )
    .unwrap();
    std::fs::write(
        project_root.join("src/hooks/useQuotes.ts"),
        "import { getQuotes } from '../lib/api/quotes';\n",
    )
    .unwrap();

    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag")
        .expect("Cargo should expose the bpx-rag executable to integration tests");
    let mut child = Command::new(executable)
        .arg("serve")
        .current_dir(project.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", &data_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{
                "name":"bpx_file_relationships",
                "arguments":{"path":"src/lib/api/quotes.ts","root":"project"}
            }
        }),
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses = String::from_utf8(output.stdout).unwrap();
    let parsed = responses
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_list = &parsed[1]["result"]["tools"];
    assert_eq!(tool_list.as_array().unwrap().len(), 1);
    assert_eq!(tool_list[0]["name"], "bpx_file_relationships");

    let result_text = parsed[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(result_text.contains("Incoming (1)"), "{result_text}");
    assert!(result_text.contains("src/hooks/useQuotes.ts"));
    assert!(result_text.contains("line 1"));
    assert!(
        !data_dir.exists(),
        "relationship-only MCP call must not create runtime data"
    );
    assert!(
        !config_dir.exists(),
        "MCP startup must not create config files"
    );
    assert!(
        !cache_dir.exists(),
        "MCP startup must not create cache files"
    );
}

#[test]
fn cli_analyze_file_scans_current_files_without_creating_runtime_state() {
    let project = tempdir().unwrap();
    let config_dir = project.path().join("runtime-config");
    let data_dir = project.path().join("runtime-data");
    let cache_dir = project.path().join("runtime-cache");
    std::fs::create_dir_all(project.path().join("src/api")).unwrap();
    std::fs::write(
        project.path().join("src/api/quotes.ts"),
        "export function getQuotes() {}\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("src/useQuotes.ts"),
        "import { getQuotes } from './api/quotes';\n",
    )
    .unwrap();
    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap();

    let output = Command::new(executable)
        .args([
            "analyze-file",
            "src/api/quotes.ts",
            "--root",
            project.path().to_str().unwrap(),
        ])
        .current_dir(project.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("XDG_DATA_HOME", &data_dir)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Incoming (1)"));
    assert!(stdout.contains("src/useQuotes.ts"));
    assert!(
        !data_dir.exists(),
        "CLI relationship scan must not create runtime data"
    );
    assert!(
        !config_dir.exists(),
        "CLI analyzer must not create config files"
    );
    assert!(
        !cache_dir.exists(),
        "CLI analyzer must not create cache files"
    );
}

#[test]
fn cli_exposes_only_analyze_file_and_mcp_server_commands() {
    let executable = std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap();
    let output = Command::new(executable).arg("--help").output().unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("analyze-file"));
    assert!(help.contains("serve"));
    let command_lines = help
        .lines()
        .filter_map(|line| line.trim().split_once(' '))
        .map(|(command, _)| command)
        .collect::<Vec<_>>();
    assert!(!command_lines.contains(&"knowledge"));
    assert!(!command_lines.contains(&"search"));
    assert!(!command_lines.contains(&"index"));
}

#[test]
fn bpcontext_compatibility_launcher_forwards_to_bpx_rag() {
    let executable = std::env::var("CARGO_BIN_EXE_bpcontext").unwrap();
    let output = Command::new(executable).arg("--help").output().unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Filesystem-first file relationship analysis"));
    assert!(help.contains("analyze-file"));
}
