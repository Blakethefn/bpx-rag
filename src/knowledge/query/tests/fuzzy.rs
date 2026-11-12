use super::super::fuzzy::{extract_knowledge_terms, knowledge_fuzzy_search};
use super::super::lexical::knowledge_bm25_search;
use super::super::*;
use super::{make_store, seed_chunk};
use rusqlite::params;

#[test]
fn fuzzy_term_extraction_reads_the_vocab_table_created_by_the_schema() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/vocab.md",
        "Whale notes",
        "whale migration routes",
        "{}",
    );
    store
        .conn()
        .execute_batch("DROP TABLE IF EXISTS knowledge_chunks_vocab")
        .unwrap();

    assert!(extract_knowledge_terms(store.conn()).is_err());
    crate::knowledge::schema::apply_schema(store.conn()).unwrap();
    let terms = extract_knowledge_terms(store.conn()).expect("schema recreates the vocab table");

    assert!(terms.iter().any(|term| term == "whale"));
}

fn filler_word(index: usize) -> String {
    const FILLER_LETTERS: &[u8; 8] = b"bcdfghjk";
    (0..6)
        .rev()
        .map(|place| char::from(FILLER_LETTERS[(index >> (3 * place)) & 7]))
        .collect()
}

fn seed_filler_vocabulary(store: &KnowledgeStore, filler_count: usize) {
    let fillers: Vec<String> = (0..filler_count).map(filler_word).collect();
    for batch in fillers.chunks(500) {
        store
            .conn()
            .execute(
                "INSERT INTO knowledge_chunks (title, content) VALUES ('', ?1)",
                params![batch.join(" ")],
            )
            .unwrap();
    }
}

fn content_vocab_size(store: &KnowledgeStore) -> i64 {
    let conn = store.conn();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_vocab \
         USING fts5vocab(knowledge_chunks, instance);",
    )
    .unwrap();
    conn.query_row(
        "SELECT COUNT(DISTINCT term) FROM knowledge_chunks_vocab WHERE col = 'content'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

pub(super) fn fuzzy_hits(store: &KnowledgeStore, query: &str) -> Vec<(i64, String)> {
    knowledge_fuzzy_search(store, query, 10, "", &[])
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|result| (result.rowid, result.file_path))
        .collect()
}

pub(super) fn bm25_hits(store: &KnowledgeStore, query: &str) -> Vec<(i64, String)> {
    knowledge_bm25_search(store.conn(), query, 10, "", &[])
        .unwrap()
        .into_iter()
        .map(|result| (result.rowid, result.file_path))
        .collect()
}

#[test]
fn fuzzy_corrects_misspelling_when_vocabulary_has_fewer_than_500_terms() {
    let store = make_store();
    let apple = seed_chunk(
        &store,
        "repo",
        "/repo",
        "fruit.md",
        "Fruit",
        "ripe apple",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "aple"),
        vec![(apple, "fruit.md".to_string())]
    );
}

#[test]
fn fuzzy_corrects_misspelling_of_term_that_sorts_first_in_vocabulary_above_500_terms() {
    let store = make_store();
    seed_filler_vocabulary(&store, 600);
    let aardvark = seed_chunk(
        &store,
        "repo",
        "/repo",
        "animals.md",
        "Animals",
        "aardvark",
        "{}",
    );

    assert_eq!(
        fuzzy_hits(&store, "aardvrk"),
        vec![(aardvark, "animals.md".to_string())]
    );
}

#[test]
fn fuzzy_corrects_misspelling_of_late_alphabet_term_in_vocabulary_above_500_terms() {
    let store = make_store();
    seed_filler_vocabulary(&store, 600);
    let zoo = seed_chunk(&store, "repo", "/repo", "zoo.md", "Zoo", "zookeeper", "{}");

    assert_eq!(
        fuzzy_hits(&store, "zookeepr"),
        vec![(zoo, "zoo.md".to_string())]
    );
}

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
