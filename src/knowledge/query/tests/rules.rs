use super::super::lexical::knowledge_identifier_search;
use super::super::rank::RankedResult;
use super::super::*;
use super::identifier::{indexed_store, GET_USER_BY_ID};

pub(super) fn file_paths_of_ranked(results: &[RankedResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect()
}

#[test]
fn identifier_layer_ignores_a_single_word_query() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "user", 10, "", &[]).unwrap();

    assert!(
        hits.is_empty(),
        "single-word query matched {:?}",
        file_paths_of_ranked(&hits)
    );
}

#[test]
fn identifier_layer_does_not_match_chunks_that_share_one_common_word() {
    let (store, _dir) = indexed_store(&[
        ("http_server.rs", "pub fn HttpStatusCode() -> u16 { 200 }\n"),
        (
            "session_helpers.py",
            "def remove_session_status(identifier):\n    return None\n",
        ),
    ]);

    let hits = knowledge_identifier_search(store.conn(), "http status code", 10, "", &[]).unwrap();

    assert_eq!(file_paths_of_ranked(&hits), ["http_server.rs"]);
}

#[test]
fn identifier_layer_needs_two_content_words_not_stopwords() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "by the id", 10, "", &[]).unwrap();

    assert!(
        hits.is_empty(),
        "one content word matched {:?}",
        file_paths_of_ranked(&hits)
    );
}

#[test]
fn identifier_layer_respects_source_label_filter() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);
    let results = knowledge_search(
        &store,
        "get user by id",
        None,
        Some("other-source"),
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert!(results.is_empty());
}

#[test]
fn identifier_layer_results_are_deterministic_across_repeated_searches() {
    let (store, _dir) = indexed_store(&[
        ("users.ts", GET_USER_BY_ID),
        (
            "accounts.ts",
            "export function getAccountById(id) { return id; }\n",
        ),
    ]);
    let run = || {
        knowledge_search(
            &store,
            "get user by id",
            None,
            None,
            10,
            &SearchWeights::default(),
            2000,
        )
        .unwrap()
        .into_iter()
        .map(|result| (result.file_path, result.score.to_bits()))
        .collect::<Vec<_>>()
    };

    assert_eq!(run(), run());
}
