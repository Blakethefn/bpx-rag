use crate::knowledge::{enrichment, sync};
use std::fs;
use tempfile::tempdir;

// ── Filter parser → SQL → search roundtrip ─────────────────────────────

#[test]
fn sync_and_traverse_wikilinks() {
    let store = super::super::make_store();
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
    assert_eq!(super::super::link_count(&store, source.id), 3);

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
fn remove_source_cleans_up_links() {
    let store = super::super::make_store();
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
    assert!(super::super::link_count(&store, source.id) > 0);

    store.remove_source("doomed").unwrap();

    // Links should be gone (FK cascade)
    let orphaned: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_links", [], |row| row.get(0))
        .unwrap();
    assert_eq!(orphaned, 0, "links cleaned up after source removal");
}
