use super::super::lexical::{knowledge_bm25_search, knowledge_identifier_search};
use super::{make_store, seed_chunk};

fn ranked_ids(rows: &[super::super::rank::RankedResult]) -> Vec<(i64, f64)> {
    rows.iter().map(|row| (row.rowid, row.rank)).collect()
}

#[test]
fn scoped_lexical_layers_keep_global_order_and_scores() {
    let store = make_store();
    seed_chunk(
        &store,
        "outside",
        "/outside",
        "a.md",
        "Needle Alpha",
        "needle alpha appears outside the selected scope",
        "{}",
    );
    seed_chunk(
        &store,
        "selected",
        "/selected",
        "b.md",
        "Needle Alpha",
        "needle alpha appears in the selected scope",
        "{}",
    );
    seed_chunk(
        &store,
        "outside",
        "/outside",
        "c.md",
        "Needle Alpha",
        "needle alpha appears outside again",
        "{}",
    );
    seed_chunk(
        &store,
        "selected",
        "/selected",
        "d.md",
        "Needle Alpha",
        "needle alpha appears in another selected file",
        "{}",
    );

    let scope = "kcm.source_id = (SELECT id FROM knowledge_sources WHERE label = ?1)";
    let params = ["selected".to_string()];
    let expected_bm25: Vec<_> = knowledge_bm25_search(store.conn(), "needle alpha", 100, "", &[])
        .unwrap()
        .into_iter()
        .filter(|row| row.source_label == "selected")
        .map(|row| (row.rowid, row.rank))
        .collect();
    let actual_bm25 =
        knowledge_bm25_search(store.conn(), "needle alpha", 100, scope, &params).unwrap();
    assert_eq!(ranked_ids(&actual_bm25), expected_bm25);

    let expected_identifiers: Vec<_> =
        knowledge_identifier_search(store.conn(), "needle alpha", 100, "", &[])
            .unwrap()
            .into_iter()
            .filter(|row| row.source_label == "selected")
            .map(|row| (row.rowid, row.rank))
            .collect();
    let actual_identifiers =
        knowledge_identifier_search(store.conn(), "needle alpha", 100, scope, &params).unwrap();
    assert_eq!(ranked_ids(&actual_identifiers), expected_identifiers);
}
