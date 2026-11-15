use serde_json::json;
use tempfile::tempdir;

use super::client::{call, tool_text};

#[test]
fn bpx_findings_returns_partial_analysis_rows() {
    let temp = tempdir().unwrap();
    let mixed = temp.path().join("mixed");
    std::fs::create_dir_all(mixed.join("src")).unwrap();
    std::fs::write(
        mixed.join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    std::fs::write(mixed.join("src/util.rs"), "fn hidden() {}\n").unwrap();
    std::fs::write(mixed.join("src/other.rs"), "fn broken( {\n").unwrap();
    let added_mixed = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "bpx_add",
                "arguments": { "path": mixed, "label": "mixed" }
            }
        })],
    );
    assert!(
        !added_mixed[0]["result"]["isError"]
            .as_bool()
            .unwrap_or(false),
        "{}",
        tool_text(&added_mixed[0])
    );
    let partial = call(
        temp.path(),
        &[json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "bpx_findings",
                "arguments": { "source": "mixed" }
            }
        })],
    );
    let partial_text = tool_text(&partial[0]);
    assert!(
        !partial[0]["result"]["isError"].as_bool().unwrap_or(false),
        "{partial_text}"
    );
    assert!(partial_text.contains("status: partial"), "{partial_text}");
    assert!(
        partial_text
            .contains("possibly_unused_symbol src/util.rs hidden no reference in sealed scope"),
        "{partial_text}"
    );
    assert!(
        partial_text.contains("src/other.rs:unsealed"),
        "{partial_text}"
    );
    assert!(
        !partial_text.contains("unreachable_file src/other.rs"),
        "{partial_text}"
    );
    assert!(
        !partial_text.contains("possibly_unused_symbol src/other.rs"),
        "{partial_text}"
    );
}
