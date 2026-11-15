use super::super::super::*;
use super::assert_identifier_index_matches_chunks;
use crate::knowledge::identifiers::tests::maintenance::legacy;
use crate::support::{add_source, make_store};
use tempfile::tempdir;

fn symbol_hits(store: &KnowledgeStore, name: &str) -> Vec<(u32, u32)> {
    let report = crate::knowledge::symbols::lookup(
        store,
        &crate::knowledge::symbols::SymbolQuery {
            name,
            mode: crate::knowledge::symbols::MatchMode::Exact,
            source: None,
            limit: 10,
        },
    )
    .unwrap();
    report
        .hits
        .iter()
        .map(|hit| (hit.line_start, hit.line_end))
        .collect()
}

#[test]
fn legacy_content_update_hides_stale_symbol_lines_until_sync_rebuilds_them() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha_one() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    let source = add_source(&store, "identifier-maintenance", dir.path());
    sync_source(&store, &source, None).unwrap();
    assert_eq!(symbol_hits(&store, "alpha_one"), vec![(1, 3)]);
    let file_id: i64 = store
        .conn()
        .query_row(
            "SELECT id FROM knowledge_files WHERE rel_path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let updated = "\n\npub fn beta_two() -> u8 {\n    2\n}\n";
    fs::write(dir.path().join("lib.rs"), updated).unwrap();
    legacy::delete_chunks(store.conn(), file_id);
    legacy::insert_chunk(store.conn(), file_id, source.id, "lib.rs", updated);
    let hash = crate::knowledge::sync::sha256_hex(updated.as_bytes());
    legacy::set_content_hash(store.conn(), file_id, &hash);

    assert!(
        symbol_hits(&store, "alpha_one").is_empty(),
        "a line range from the previous content was cited"
    );
    assert!(
        symbol_hits(&store, "beta_two").is_empty(),
        "not indexed yet"
    );

    sync_source(&store, &source, None).unwrap();

    assert!(symbol_hits(&store, "alpha_one").is_empty());
    assert_eq!(symbol_hits(&store, "beta_two"), vec![(3, 5)]);
    assert_identifier_index_matches_chunks(&store);
}
