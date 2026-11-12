use super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use std::path::Path;
use tempfile::tempdir;

#[test]
pub(super) fn enrichment_to_filtered_search_pipeline() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("task.md"),
        "---\ntype: task\nstatus: active\ntags:\n  - implementation\n  - system3\n---\n# Active Task\n\nThis task involves building the prediction engine.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("note.md"),
        "---\ntype: note\nstatus: draft\n---\n# Draft Note\n\nSome draft notes about the prediction engine.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("plain.md"),
        "# Plain File\n\nNo frontmatter here, just content about the prediction engine.",
    )
    .unwrap();

    let enrichments = vec!["frontmatter".to_string()];
    store
        .add_source("vault", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("vault").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    let weights = SearchWeights::default();

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1, "type:task should match exactly one file");
    assert_eq!(results[0].metadata["frontmatter"]["type"], "task");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("type:project"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(results.is_empty(), "type:project should match nothing");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("tag:implementation"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(
        results.len(),
        1,
        "tag:implementation should match exactly one file"
    );
    assert_eq!(results[0].metadata["frontmatter"]["type"], "task");

    let results = query::knowledge_search(
        &store,
        "prediction engine",
        Some("status:active"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].metadata["frontmatter"]["status"], "active");

    let results =
        query::knowledge_search(&store, "prediction engine", None, None, 10, &weights, 2000)
            .unwrap();
    assert_eq!(
        results.len(),
        3,
        "unfiltered search should find all three files"
    );
}

#[test]
pub(super) fn all_enrichments_indexed_and_searchable() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::create_dir_all(dir.path().join("01-projects/myapp")).unwrap();
    std::fs::write(
        dir.path().join("01-projects/myapp/spec.md"),
        "---\ntype: spec\nstatus: active\ntags:\n  - architecture\n---\n# App Spec\n\nSee [[other-note]] for details about the waveform analyzer.",
    )
    .unwrap();

    let enrichments = vec![
        "frontmatter".to_string(),
        "wikilinks".to_string(),
        "folder_tags".to_string(),
    ];
    store
        .add_source("vault", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("vault").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    let weights = SearchWeights::default();

    let results = query::knowledge_search(
        &store,
        "waveform analyzer",
        Some("type:spec"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);

    let results = query::knowledge_search(
        &store,
        "waveform analyzer",
        Some("folder:01-projects"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);

    let results = query::knowledge_search(
        &store,
        "waveform analyzer",
        Some("tag:architecture"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);

    let metadata = &results[0].metadata;
    assert!(
        metadata.get("frontmatter").is_some(),
        "metadata should contain frontmatter"
    );
    assert!(
        metadata.get("wikilinks").is_some(),
        "metadata should contain wikilinks"
    );
    assert!(
        metadata.get("folder_tags").is_some(),
        "metadata should contain folder_tags"
    );

    let wikilinks = metadata["wikilinks"].as_array().unwrap();
    assert!(
        wikilinks.iter().any(|l| l.as_str() == Some("other-note")),
        "wikilinks should contain 'other-note'"
    );

    let folder_tags = metadata["folder_tags"].as_array().unwrap();
    assert!(
        folder_tags
            .iter()
            .any(|t| t.as_str() == Some("01-projects")),
        "folder_tags should contain '01-projects'"
    );
    assert!(
        folder_tags.iter().any(|t| t.as_str() == Some("myapp")),
        "folder_tags should contain 'myapp'"
    );
}

#[test]
pub(super) fn no_frontmatter_still_searchable() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("plain.md"),
        "# Plain Markdown\n\nNo frontmatter here, just the crystallography methodology.",
    )
    .unwrap();

    let enrichments = vec!["frontmatter".to_string()];
    store
        .add_source("vault", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("vault").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "crystallography", None, None, 10, &weights, 2000).unwrap();
    assert!(
        !results.is_empty(),
        "file without frontmatter should still be searchable by content"
    );
}
