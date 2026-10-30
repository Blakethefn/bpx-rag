use super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{enrichment, query, sync};
use rusqlite::params;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
pub(super) fn full_sync_crud_cycle() {
    let store = make_store();
    let dir = tempdir().unwrap();

    // Create 10 files
    for i in 0..10 {
        std::fs::write(
            dir.path().join(format!("note{i}.md")),
            format!("---\ntype: note\nindex: {i}\n---\n# Note {i}\n\nContent for note {i}."),
        )
        .unwrap();
    }

    // Register and initial sync
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

    // Verify source stats updated
    let source = store.get_source("vault").unwrap().unwrap();
    assert_eq!(source.file_count, 10);
    assert!(source.chunk_count > 0);
    assert!(source.last_sync.is_some());

    // Modify 2, delete 1, add 1
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

    // Verify file count: 10 - 1 + 1 = 10
    let source = store.get_source("vault").unwrap().unwrap();
    assert_eq!(source.file_count, 10);

    // Verify modified content is present in chunks
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
            .filter_map(|r| r.ok())
            .collect()
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

// ── Test Anchor 2: Frontmatter extraction + filtered search ────────────
// "Index file with YAML frontmatter. Search with filter type:task → match.
//  Search type:project → no match. Search tag:foo → match."

#[test]
pub(super) fn sync_after_new_file_is_searchable() {
    let store = make_store();
    let dir = tempdir().unwrap();

    // Initial file
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

    // Write a new file (simulating Write tool)
    let new_path = dir.path().join("new-feature.md");
    std::fs::write(
        &new_path,
        "# New Feature\n\nImplementing the xylophone integration module.",
    )
    .unwrap();

    // Sync after a single file changes
    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    // Search should find the new content immediately
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

// ── Test Anchor 7: Re-sync after an edit updates search results ───────────
// "Edit existing indexed file. Reindex. New content appears, old gone."

#[test]
pub(super) fn sync_after_edit_updates_content() {
    let store = make_store();
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

    // Verify original content is searchable
    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(!results.is_empty(), "original content should be searchable");

    // Edit the file (simulating Edit tool)
    std::fs::write(
        &file_path,
        "# Beta Version\n\nReplacement nebula framework with improved throughput.",
    )
    .unwrap();

    let source = store.get_source("docs").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    // New content is searchable
    let results =
        query::knowledge_search(&store, "nebula framework", None, None, 10, &weights, 2000)
            .unwrap();
    assert!(
        !results.is_empty(),
        "updated content should be searchable after sync"
    );

    // Old content is no longer present
    let results =
        query::knowledge_search(&store, "quasar protocol", None, None, 10, &weights, 2000).unwrap();
    assert!(results.is_empty(), "old content should be gone after sync");
}

// ── Invariant 2: Cascading deletion ────────────────────────────────────
// "Removing a source deletes all files, chunks, chunk_meta, embeddings."

#[test]
pub(super) fn cascading_deletion_leaves_zero_orphans() {
    let store = make_store_with_embedder();
    let dir = tempdir().unwrap();

    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("f{i}.md")),
            format!("---\ntype: task\n---\n# File {i}\n\nContent for file {i} with unique terms.",),
        )
        .unwrap();
    }

    let enrichments = vec!["frontmatter".to_string()];
    store
        .add_source("doomed", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("doomed").unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, dir.path());
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    // Verify data exists before removal
    let source = store.get_source("doomed").unwrap().unwrap();
    assert!(source.file_count > 0);
    assert!(source.chunk_count > 0);

    let embed_count_before: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        embed_count_before > 0,
        "embeddings should exist before removal"
    );

    // Remove the source
    let result = store.remove_source("doomed").unwrap();
    assert!(result.files_removed > 0);
    assert!(result.chunks_removed > 0);

    // Verify zero orphans in every table
    let orphaned_files: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(orphaned_files, 0, "no files should remain");

    let orphaned_chunks: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunks", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(orphaned_chunks, 0, "no FTS5 chunks should remain");

    let orphaned_trigram: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunks_trigram", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(orphaned_trigram, 0, "no trigram chunks should remain");

    let orphaned_meta: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunk_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(orphaned_meta, 0, "no chunk_meta should remain");

    let orphaned_embeds: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(orphaned_embeds, 0, "no embeddings should remain");

    let orphaned_sources: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_sources", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(orphaned_sources, 0, "no sources should remain");
}

// ── Invariant 4: Idempotent sync ───────────────────────────────────────
// "Running sync twice with no changes produces identical state."

#[test]
pub(super) fn idempotent_sync_no_changes() {
    let store = make_store();
    let dir = tempdir().unwrap();

    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("s{i}.md")),
            format!("# Stable {i}\n\nUnchanging content."),
        )
        .unwrap();
    }

    store
        .add_source("stable", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("stable").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let source = store.get_source("stable").unwrap().unwrap();
    let chunks_after_first: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();

    // Second sync — no file changes
    let r2 = sync::sync_source(&store, &source, None).unwrap();
    assert_eq!(r2.files_unchanged, 5);
    assert_eq!(r2.files_added, 0);
    assert_eq!(r2.files_updated, 0);
    assert_eq!(r2.files_removed, 0);

    let chunks_after_second: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        chunks_after_first, chunks_after_second,
        "chunk count should be identical after idempotent sync"
    );
}

// ── Test Anchor 4: Empty knowledge graceful handling ───────────────────
// "With no sources registered, search returns empty, not error."

#[test]
pub(super) fn sync_with_enrichments_updates_metadata() {
    let store = make_store();
    let dir = tempdir().unwrap();

    let file_path = dir.path().join("evolving.md");
    std::fs::write(
        &file_path,
        "---\ntype: task\nstatus: active\n---\n# Task\n\nWorking on the quantum simulator.",
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

    // Verify initial state: searchable with type:task
    let weights = SearchWeights::default();
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);

    // Change the type from task → spec
    std::fs::write(
        &file_path,
        "---\ntype: spec\nstatus: done\n---\n# Spec\n\nCompleted quantum simulator specification.",
    )
    .unwrap();

    let source = store.get_source("vault").unwrap().unwrap();
    sync::sync_source(&store, &source, enrich_ref).unwrap();

    // Old filter should no longer match
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert!(
        results.is_empty(),
        "type:task should no longer match after status change"
    );

    // New filter should match
    let results = query::knowledge_search(
        &store,
        "quantum simulator",
        Some("type:spec"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1, "type:spec should match after sync");
    assert_eq!(results[0].metadata["frontmatter"]["status"], "done");
}

// ── Boundary: source path no longer exists ─────────────────────────────

#[test]
pub(super) fn sync_nonexistent_source_path_errors_gracefully() {
    let store = make_store();
    let dir = tempdir().unwrap();
    let path_str = dir.path().to_str().unwrap().to_string();

    store.add_source("temp", &path_str, &[], &[]).unwrap();
    let source = store.get_source("temp").unwrap().unwrap();

    // Remove the directory
    drop(dir);

    // Sync should error but not panic
    let result = sync::sync_source(&store, &source, None);
    assert!(
        result.is_err(),
        "sync should error when source path is gone"
    );
}

// ── Sync with embedder generates searchable vectors ────────────────────

#[test]
pub(super) fn sync_with_embedder_produces_embeddings() {
    let store = make_store_with_embedder();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("embed.md"),
        "# Embedding Test\n\nVector embeddings for semantic search.",
    )
    .unwrap();

    store
        .add_source("emb", dir.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("emb").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let embed_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        embed_count > 0,
        "embeddings should be generated during sync"
    );

    // Every chunk should have a corresponding embedding
    let chunk_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_chunk_meta", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        chunk_count, embed_count,
        "every chunk should have an embedding"
    );
}

// ── Filter parser → SQL → search roundtrip ─────────────────────────────

#[test]
pub(super) fn sync_and_traverse_wikilinks() {
    let store = make_store();
    let dir = tempdir().unwrap();

    // a.md links to b.md and c.md; b.md links back to a.md
    fs::write(dir.path().join("a.md"), "# Note A\nSee [[b]] and [[c]]").unwrap();
    fs::write(dir.path().join("b.md"), "# Note B\nBack to [[a]]").unwrap();
    fs::write(dir.path().join("c.md"), "# Note C\nStandalone").unwrap();

    let enrichments = vec!["wikilinks".to_string()];
    store
        .add_source("graph", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("graph").unwrap().unwrap();

    let enrich_fn =
        enrichment::build_enrichment_fn(&source.enrichments, std::path::Path::new(&source.path));
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &std::path::Path) -> serde_json::Value);
    let result = sync::sync_source(&store, &source, enrich_ref).unwrap();

    // Should have created links: a→b, a→c, b→a
    assert_eq!(result.links_created, 3, "a→b, a→c, b→a");
    assert_eq!(link_count(&store, source.id), 3);

    // Find file_id for a.md
    let a_file_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = 'a.md'",
            rusqlite::params![source.id],
            |row| row.get(0),
        )
        .unwrap();

    // Both directions from a: chunks from b.md (a links to b, b links to a)
    // and c.md (a links to c).
    let related = store.get_related_chunks(a_file_id, 50, None).unwrap();
    assert!(related.len() >= 2, "a links to b and c");
    assert!(related.iter().any(|c| c.rel_path == "c.md"));
    assert!(related.iter().any(|c| c.rel_path == "b.md"));
}

#[test]
pub(super) fn remove_source_cleans_up_links() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("x.md"), "Link to [[y]]").unwrap();
    fs::write(dir.path().join("y.md"), "# Y").unwrap();

    let enrichments = vec!["wikilinks".to_string()];
    store
        .add_source("doomed", dir.path().to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source("doomed").unwrap().unwrap();

    let enrich_fn =
        enrichment::build_enrichment_fn(&source.enrichments, std::path::Path::new(&source.path));
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &std::path::Path) -> serde_json::Value);
    sync::sync_source(&store, &source, enrich_ref).unwrap();
    assert!(link_count(&store, source.id) > 0);

    store.remove_source("doomed").unwrap();

    // Links should be gone (FK cascade)
    let orphaned: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_links", [], |row| row.get(0))
        .unwrap();
    assert_eq!(orphaned, 0, "links cleaned up after source removal");
}
