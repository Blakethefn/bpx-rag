use super::super::*;
use crate::support::make_store;
use tempfile::tempdir;

mod resolve;
mod resync;

fn register_source_with_enrichments(
    store: &KnowledgeStore,
    dir: &Path,
    enrichments: &[&str],
) -> KnowledgeSourceInfo {
    let e: Vec<String> = enrichments.iter().map(|s| s.to_string()).collect();
    store
        .add_source("test", dir.to_str().unwrap(), &[], &e)
        .unwrap();
    store.get_source("test").unwrap().unwrap()
}

fn link_count(store: &KnowledgeStore, source_id: i64) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_links
             WHERE source_file_id IN (SELECT id FROM knowledge_files WHERE source_id = ?1)",
            rusqlite::params![source_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn sync_creates_wikilink_edges() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "---\ntype: note\n---\nSee [[b]]").unwrap();
    fs::write(dir.path().join("b.md"), "# B\nTarget note").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn = super::super::super::enrichment::build_enrichment_fn(
        &source.enrichments,
        Path::new(&source.path),
    );
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    let result = sync_source(&store, &source, enrich_ref).unwrap();

    assert!(result.links_created > 0, "should create wikilink edges");
    assert!(link_count(&store, source.id) > 0);
}

#[test]
fn sync_no_self_links() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("self.md"), "See [[self]]").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn = super::super::super::enrichment::build_enrichment_fn(
        &source.enrichments,
        Path::new(&source.path),
    );
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync_source(&store, &source, enrich_ref).unwrap();

    assert_eq!(
        link_count(&store, source.id),
        0,
        "self-links should be skipped"
    );
}

#[test]
fn sync_unresolvable_links_ignored() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "Link to [[nonexistent]]").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn = super::super::super::enrichment::build_enrichment_fn(
        &source.enrichments,
        Path::new(&source.path),
    );
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync_source(&store, &source, enrich_ref).unwrap();

    assert_eq!(link_count(&store, source.id), 0);
}
