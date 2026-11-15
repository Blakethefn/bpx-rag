use super::super::{make_store, seed_chunk};
use super::{bm25_hits, content_vocab_size, fuzzy_hits, seed_filler_vocabulary};

#[test]
fn fuzzy_correction_is_the_same_at_499_500_501_and_5000_terms() {
    for vocab_size in [499, 500, 501, 5000] {
        let store = make_store();
        seed_filler_vocabulary(&store, vocab_size - 1);
        let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

        assert_eq!(
            content_vocab_size(&store),
            vocab_size as i64,
            "vocabulary size for case {vocab_size}"
        );
        assert_eq!(
            fuzzy_hits(&store, "zookeepr"),
            vec![(zoo, "zoo.md".to_string())],
            "misspelling must be corrected at vocabulary size {vocab_size}"
        );
    }
}

#[test]
fn fuzzy_leaves_exact_match_words_unchanged() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "cat.md", "Cat", "cat", "{}");
    seed_chunk(&store, "repo", "/repo", "cot.md", "Cot", "cot", "{}");

    assert!(
        fuzzy_hits(&store, "cat").is_empty(),
        "an exact match must not be rewritten to a neighbour"
    );
    assert!(
        fuzzy_hits(&store, "Cat").is_empty(),
        "a case-only difference from an indexed term is an exact match"
    );
}

#[test]
fn fuzzy_leaves_words_without_a_candidate_within_distance_two_unchanged() {
    let store = make_store();
    seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    assert!(fuzzy_hits(&store, "qwzxv").is_empty());
    assert!(fuzzy_hits(&store, "zkeepr").is_empty());
    assert_eq!(
        fuzzy_hits(&store, "zookeepr qwzxv"),
        bm25_hits(&store, "zookeep qwzxv")
    );
}

#[test]
fn fuzzy_corrects_within_distance_two() {
    let store = make_store();
    let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    assert_eq!(
        fuzzy_hits(&store, "zokeepr"),
        vec![(zoo, "zoo.md".to_string())]
    );
}
