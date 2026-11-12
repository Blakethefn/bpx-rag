use crate::knowledge::{enrichment, sync};
use std::path::Path;
use tempfile::tempdir;

#[test]
fn cascading_deletion_leaves_zero_orphans() {
    let store = super::super::make_store_with_embedder();
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

    let result = store.remove_source("doomed").unwrap();
    assert!(result.files_removed > 0);
    assert!(result.chunks_removed > 0);

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
