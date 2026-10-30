use super::*;
use crate::knowledge::{self, enrichment, sync};

#[test]
fn top_result_lists_files_linked_by_wikilinks() {
    let store = knowledge::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("guide.md"),
        "# Guide\n\nThe kestrel handbook points to [[appendix]].",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("appendix.md"),
        "# Appendix\n\nunrelated words",
    )
    .unwrap();
    store
        .add_source(
            "docs",
            dir.path().to_str().unwrap(),
            &[],
            &["wikilinks".to_string()],
        )
        .unwrap();
    let source = store.get_source("docs").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&source.enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &std::path::Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    let request = QueryRequest {
        query: "kestrel",
        source: None,
        knowledge_filter: None,
        limit: 5,
        snippet_bytes: 500,
        profile: SearchProfile::Default,
        explain: false,
    };
    let output = run_query(request, &Config::default(), &store, false).unwrap();

    assert!(
        output.contains("file: guide.md"),
        "guide.md is the top hit: {output}"
    );
    assert!(
        output.contains("related: appendix.md"),
        "linked file is listed under the hit: {output}"
    );
}

fn rust_relation_store() -> crate::knowledge::KnowledgeStore {
    let store = knowledge::open_in_memory().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("lib.rs"),
        "mod util;\n// kestrel dispatch entry point\npub fn run() {}\n",
    )
    .unwrap();
    std::fs::write(root.join("util.rs"), "pub fn helper() -> u8 { 1 }\n").unwrap();
    store
        .add_source("crate", root.to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("crate").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();
    store
}

fn default_request(query: &str) -> QueryRequest<'_> {
    QueryRequest {
        query,
        source: None,
        knowledge_filter: None,
        limit: 5,
        snippet_bytes: 500,
        profile: SearchProfile::Default,
        explain: false,
    }
}

#[test]
fn default_search_adds_the_file_a_top_hit_imports() {
    let store = rust_relation_store();
    let output = run_query(
        default_request("kestrel"),
        &Config::default(),
        &store,
        false,
    )
    .unwrap();

    assert!(
        output.contains("file: lib.rs"),
        "lib.rs is the match: {output}"
    );
    assert!(
        output.contains("file: util.rs"),
        "util.rs is a module the match declares, so it is merged in: {output}"
    );
    assert!(
        output.contains("graph-neighbor"),
        "neighbor is labelled: {output}"
    );
}

#[test]
fn relation_neighbors_can_be_turned_off() {
    let store = rust_relation_store();
    let mut config = Config::default();
    config.search.relation_neighbors = 0;
    let output = run_query(default_request("kestrel"), &config, &store, false).unwrap();

    assert!(
        !output.contains("file: util.rs"),
        "no neighbors when disabled: {output}"
    );
}
