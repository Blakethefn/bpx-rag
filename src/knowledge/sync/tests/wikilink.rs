use super::super::index::resolve_wikilink;
use super::super::*;
use crate::support::{add_source, make_store};
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn resolve_exact_path() {
    let files: HashMap<String, i64> = [("01-projects/bpcontext.md".into(), 1)]
        .into_iter()
        .collect();
    assert_eq!(
        resolve_wikilink("01-projects/bpcontext.md", &files),
        Some(1)
    );
}

#[test]
fn resolve_appends_md_extension() {
    let files: HashMap<String, i64> = [("notes/idea.md".into(), 2)].into_iter().collect();
    assert_eq!(resolve_wikilink("notes/idea", &files), Some(2));
}

#[test]
fn resolve_basename_match() {
    let files: HashMap<String, i64> = [("deep/path/my-note.md".into(), 3)].into_iter().collect();
    assert_eq!(resolve_wikilink("my-note", &files), Some(3));
}

#[test]
fn resolve_basename_without_extension() {
    let files: HashMap<String, i64> = [("folder/config".into(), 4)].into_iter().collect();
    assert_eq!(resolve_wikilink("config", &files), Some(4));
}

#[test]
fn resolve_returns_none_for_missing() {
    let files: HashMap<String, i64> = [("a.md".into(), 1)].into_iter().collect();
    assert_eq!(resolve_wikilink("nonexistent", &files), None);
}

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

#[test]
fn sync_resync_updates_links() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "See [[b]]").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn = super::super::super::enrichment::build_enrichment_fn(
        &source.enrichments,
        Path::new(&source.path),
    );
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync_source(&store, &source, enrich_ref).unwrap();
    assert_eq!(link_count(&store, source.id), 1, "a→b");

    fs::write(dir.path().join("a.md"), "See [[c]]").unwrap();
    let source = store.get_source("test").unwrap().unwrap();
    let enrich_fn = super::super::super::enrichment::build_enrichment_fn(
        &source.enrichments,
        Path::new(&source.path),
    );
    let enrich_ref = enrich_fn
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync_source(&store, &source, enrich_ref).unwrap();

    assert_eq!(link_count(&store, source.id), 1, "a→c only");
}

#[test]
fn sync_without_enrichments_creates_no_links() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "See [[b]]").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();

    assert_eq!(
        link_count(&store, source.id),
        0,
        "no enrichments = no links"
    );
}

#[test]
fn wikilink_resolution_reports_malformed_chunk_metadata() {
    let store = crate::knowledge::open_in_memory().unwrap();
    let source_id = store.add_source("vault", "/vault", &[], &[]).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
             VALUES (?1, 'note.md', 'hash', 1)",
            params![source_id],
        )
        .unwrap();
    let file_id = store.conn().last_insert_rowid();
    store
        .conn()
        .execute(
            "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
             VALUES (1, ?1, ?2, 1, 1, '{not json')",
            params![file_id, source_id],
        )
        .unwrap();

    let result = super::super::index::resolve_wikilinks_for_source(store.conn(), source_id);

    assert!(
        result.is_err(),
        "malformed metadata must not drop wikilinks silently"
    );
}
