use super::dispatch_tool;
use serde_json::json;
use std::path::Path;

#[test]
fn unknown_tool_is_rejected_without_opening_the_store() {
    let request = json!({
        "params": {
            "name": "bpx_missing",
            "arguments": {}
        }
    });
    let mut app = None;
    let response = dispatch_tool(&request, Path::new("."), &mut app, Some(json!(7)));
    assert_eq!(response["id"], 7);
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["content"][0]["text"],
        "Error: unknown tool: bpx_missing"
    );
    assert!(app.is_none());
}

#[test]
fn missing_tool_name_is_rejected() {
    let request = json!({ "params": { "arguments": {} } });
    let mut app = None;
    let response = dispatch_tool(&request, Path::new("."), &mut app, Some(json!(1)));
    assert_eq!(
        response["result"]["content"][0]["text"],
        "Error: unknown tool: "
    );
    assert!(app.is_none());
}
