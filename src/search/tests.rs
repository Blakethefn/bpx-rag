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
