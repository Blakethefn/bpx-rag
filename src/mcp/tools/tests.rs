use super::*;
use serde_json::json;

fn tool<'a>(definitions: &'a Value, name: &str) -> &'a Value {
    definitions["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn names(definitions: &Value) -> Vec<&str> {
    definitions["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect()
}

#[test]
fn exposes_rag_and_direct_relationship_tools() {
    let definitions = tool_definitions();
    assert_eq!(
        names(&definitions),
        [
            "bpx_search",
            "bpx_add",
            "bpx_sync",
            "bpx_status",
            "bpx_remove",
            "bpx_read",
            "bpx_models",
            "bpx_embed",
            "bpx_symbol",
            "bpx_findings",
            "bpx_watch",
            "bpx_relate",
        ]
    );
    for name in names(&definitions) {
        assert!(super::tool_by_name(name).is_some(), "{name}");
    }
    assert!(super::tool_by_name("bpx_missing").is_none());
}

#[test]
fn schema_enums_come_from_the_action_and_profile_tables() {
    let definitions = tool_definitions();
    assert_eq!(
        tool(&definitions, "bpx_search")["inputSchema"]["properties"]["profile"]["enum"],
        json!(["code_review", "debug", "planning", "vault_navigation"])
    );
    assert_eq!(
        tool(&definitions, "bpx_models")["inputSchema"]["properties"]["action"]["enum"],
        json!(["status", "download"])
    );
    assert_eq!(
        tool(&definitions, "bpx_watch")["inputSchema"]["properties"]["action"]["enum"],
        json!(["on", "off", "status"])
    );
    assert_eq!(
        tool(&definitions, "bpx_add")["inputSchema"]["required"],
        json!(["path", "label"])
    );
}
