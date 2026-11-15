use super::super::super::*;
use super::super::{make_store, seed_chunk};

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
