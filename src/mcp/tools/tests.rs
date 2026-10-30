use super::*;

#[test]
fn exposes_rag_and_direct_relationship_tools() {
    let definitions = tool_definitions();
    let names = definitions["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"bpx_search"));
    assert!(names.contains(&"bpx_add"));
    assert!(names.contains(&"bpx_sync"));
    assert!(names.contains(&"bpx_status"));
    assert!(names.contains(&"bpx_read"));
    assert!(names.contains(&"bpx_models"));
    assert!(names.contains(&"bpx_embed"));
    assert!(names.contains(&"bpx_relate"));
}
