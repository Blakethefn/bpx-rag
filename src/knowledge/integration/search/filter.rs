use super::super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use std::path::Path;
use tempfile::tempdir;

#[test]
pub(super) fn filter_never_returns_non_matching_chunks() {
    let store = make_store();
    let dir = tempdir().unwrap();

    for (i, ftype) in ["task", "note", "spec", "output", "log"].iter().enumerate() {
        std::fs::write(
            dir.path().join(format!("{ftype}.md")),
            format!(
                "---\ntype: {ftype}\n---\n# {ftype} {i}\n\nContent about the electron microscope.",
            ),
        )
        .unwrap();
    }

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
        "electron microscope",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    for result in &results {
        assert_eq!(
            result.metadata["frontmatter"]["type"], "task",
            "every filtered result must match the filter predicate"
        );
    }
    assert_eq!(results.len(), 1, "exactly one task file should match");
}

#[test]
pub(super) fn compound_filter_roundtrip() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
    std::fs::write(
        dir.path().join("tasks/match.md"),
        "---\ntype: task\nstatus: active\ntags:\n  - urgent\n---\n# Matching Task\n\nContent about the spectral analysis.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tasks/mismatch-type.md"),
        "---\ntype: note\nstatus: active\ntags:\n  - urgent\n---\n# Mismatch Type\n\nContent about the spectral analysis.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tasks/mismatch-tag.md"),
        "---\ntype: task\nstatus: active\ntags:\n  - low\n---\n# Mismatch Tag\n\nContent about the spectral analysis.",
    )
    .unwrap();

    let enrichments = vec!["frontmatter".to_string(), "folder_tags".to_string()];
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
        "spectral analysis",
        Some("type:task tag:urgent folder:tasks"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(
        results.len(),
        1,
        "compound filter should match exactly one file"
    );
    assert_eq!(results[0].title, "Matching Task");
}
