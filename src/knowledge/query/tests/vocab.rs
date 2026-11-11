use super::super::*;
use super::{make_store, seed_chunk};
use super::fuzzy::fuzzy_hits;
use rusqlite::params;

/// Wrap a connection the way `open_in_memory` does, for tests that need two
/// connections to one file.
fn store_over(conn: rusqlite::Connection) -> KnowledgeStore {
    KnowledgeStore {
        conn,
        embedder: None,
        vector_index_dir: None,
        vector_index: None,
        vector_index_state: None,
        fuzzy_cache: Default::default(),
        query_embedding_cache: Default::default(),
    }
}

#[test]
fn fuzzy_correction_sees_terms_added_after_an_earlier_search() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");
    assert!(fuzzy_hits(&store, "quokkaa").is_empty());

    let quokka = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_correction_forgets_terms_removed_after_an_earlier_search() {
    let store = make_store();
    let quokka = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );
    assert_eq!(
        fuzzy_hits(&store, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );

    store
        .conn()
        .execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![quokka],
        )
        .unwrap();

    assert!(fuzzy_hits(&store, "quokkaa").is_empty());
}

#[test]
fn fuzzy_correction_sees_terms_written_by_another_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    let reader = store_over(crate::db::open_db(&path).unwrap());
    crate::knowledge::schema::apply_schema(reader.conn()).unwrap();
    seed_chunk(&reader, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");
    assert!(fuzzy_hits(&reader, "quokkaa").is_empty());

    let writer = store_over(crate::db::open_db(&path).unwrap());
    let quokka = seed_chunk(
        &writer,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "quokka",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&reader, "quokkaa"),
        vec![(quokka, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_repeated_runs_return_identical_corrections_for_tied_candidates() {
    let store = make_store();
    let bag = seed_chunk(&store, "repo", "/repo", "bag.md", "Bag", "bag", "{}");
    seed_chunk(&store, "repo", "/repo", "bar.md", "Bar", "bar", "{}");

    // "bag" and "bar" are both distance 1 from "bat". The smaller term wins.
    let first = fuzzy_hits(&store, "bat");
    assert_eq!(first, vec![(bag, "bag.md".to_string())]);
    for _ in 0..20 {
        assert_eq!(fuzzy_hits(&store, "bat"), first);
    }
}

#[test]
fn fuzzy_results_respect_source_and_metadata_filters() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "a.md",
        "A",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"task"}}"#,
    );
    seed_chunk(
        &store,
        "other",
        "/other",
        "b.md",
        "B",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"task"}}"#,
    );
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "c.md",
        "C",
        "zookeeper notes",
        r#"{"frontmatter":{"type":"note"}}"#,
    );
    let weights = SearchWeights {
        keyword_weight: 1.0,
        vector_weight: 0.0,
    };
    let paths = |results: Vec<KnowledgeSearchResult>| -> Vec<String> {
        let mut paths: Vec<String> = results.into_iter().map(|r| r.file_path).collect();
        paths.sort();
        paths
    };

    let by_source =
        knowledge_search(&store, "zookeepr", None, Some("repo"), 10, &weights, 2000).unwrap();
    assert_eq!(paths(by_source), vec!["a.md", "c.md"]);

    let by_metadata = knowledge_search(
        &store,
        "zookeepr",
        Some("type:task"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(paths(by_metadata), vec!["a.md", "b.md"]);

    let both = knowledge_search(
        &store,
        "zookeepr",
        Some("type:task"),
        Some("repo"),
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(paths(both), vec!["a.md"]);
}
