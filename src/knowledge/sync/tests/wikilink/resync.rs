use super::super::super::*;
use super::{link_count, register_source_with_enrichments};
use crate::support::{add_source, make_store};
use tempfile::tempdir;

#[test]
fn sync_resync_updates_links() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "See [[b]]").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let source = register_source_with_enrichments(&store, dir.path(), &["wikilinks"]);
    let enrich_fn = super::super::super::super::enrichment::build_enrichment_fn(
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
    let enrich_fn = super::super::super::super::enrichment::build_enrichment_fn(
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

    let result = super::super::super::index::resolve_wikilinks_for_source(store.conn(), source_id);

    assert!(
        result.is_err(),
        "malformed metadata must not drop wikilinks silently"
    );
}
