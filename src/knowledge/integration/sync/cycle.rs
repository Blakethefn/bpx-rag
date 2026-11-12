use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use rusqlite::params;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn full_sync_crud_cycle() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();

    for i in 0..10 {
        std::fs::write(
            dir.path().join(format!("note{i}.md")),
            format!("---\ntype: note\nindex: {i}\n---\n# Note {i}\n\nContent for note {i}."),
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

    let r1 = sync::sync_source(&store, &source, enrich_ref).unwrap();
    assert_eq!(r1.files_added, 10);
    assert_eq!(r1.files_unchanged, 0);
    assert!(r1.chunks_total > 0);

    let source = store.get_source("vault").unwrap().unwrap();
    assert_eq!(source.file_count, 10);
    assert!(source.chunk_count > 0);
    assert!(source.last_sync.is_some());

    std::fs::write(
        dir.path().join("note0.md"),
        "---\ntype: note\nindex: 0\n---\n# Note 0\n\nModified content for note zero.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("note1.md"),
        "---\ntype: task\nindex: 1\n---\n# Task 1\n\nConverted to task type.",
    )
    .unwrap();
    std::fs::remove_file(dir.path().join("note9.md")).unwrap();
    std::fs::write(
        dir.path().join("note10.md"),
        "---\ntype: note\nindex: 10\n---\n# Note 10\n\nBrand new note.",
    )
    .unwrap();

    let source = store.get_source("vault").unwrap().unwrap();
    let r2 = sync::sync_source(&store, &source, enrich_ref).unwrap();
    assert_eq!(r2.files_added, 1);
    assert_eq!(r2.files_updated, 2);
    assert_eq!(r2.files_removed, 1);
    assert_eq!(r2.files_unchanged, 7);

    let source = store.get_source("vault").unwrap().unwrap();
    assert_eq!(source.file_count, 10);

    let chunks: Vec<String> = {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT kc.content FROM knowledge_chunks kc
                 JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
                 WHERE kcm.source_id = ?1",
            )
            .unwrap();
        stmt.query_map(params![source.id], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .expect("every indexed chunk decodes")
    };
    let all_content = chunks.join("\n");
    assert!(
        all_content.contains("Modified content for note zero"),
        "updated content should be present"
    );
    assert!(
        all_content.contains("Brand new note"),
        "new file content should be present"
    );
}

#[test]
fn sync_after_new_file_is_searchable() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("existing.md"),
        "# Existing\n\nOriginal content about authentication.",
    )
    .unwrap();

    store
        .add_source("docs", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let new_path = dir.path().join("new-feature.md");
    std::fs::write(
        &new_path,
        "# New Feature\n\nImplementing the xylophone integration module.",
    )
    .unwrap();

    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let weights = SearchWeights::default();
    let results = query::knowledge_search(
        &store,
        "xylophone integration",
        None,
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(
        !results.is_empty(),
        "newly synced file should be searchable immediately"
    );
    assert!(results[0].snippet.contains("xylophone"));
}

#[test]
fn sync_after_edit_updates_content() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();

    let file_path = dir.path().join("evolving.md");
    std::fs::write(
        &file_path,
        "# Alpha Version\n\nOriginal implementation of the quasar protocol.",
    )
    .unwrap();

    store
        .add_source("docs", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty(), "original content should be searchable");

    std::fs::write(
        &file_path,
        "# Beta Version\n\nReplacement nebula framework with improved throughput.",
    )
    .unwrap();

    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let results =
        query::knowledge_search(&store, "nebula framework", None, None, 10, &weights, 2000)
            .unwrap();
    assert!(
        !results.is_empty(),
        "updated content should be searchable after sync"
    );

    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty(), "old content should be gone after sync");
}

#[test]
fn sync_nonexistent_source_path_errors_gracefully() {
    let store = super::super::make_store();
    let dir = tempdir().unwrap();
    let path_str = dir.path().to_str().unwrap().to_string();

    store.add_source("temp", &path_str, &[], &[]).unwrap();
    let source = store.get_source("temp").unwrap().unwrap();

    drop(dir);

    let result = sync::sync_source(&store, &source, None);
    assert!(
        result.is_err(),
        "sync should error when source path is gone"
    );
}
