use super::super::*;
use super::identifier::{GET_USER_BY_ID, indexed_store};

pub(super) fn file_paths_of_ranked(results: &[RankedResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect()
}

// ── identifier layer: selective matching ─────────────────────────────────

#[test]
fn identifier_layer_ignores_a_single_word_query() {
    // "user" is one content word, so no identifier match is attempted at all.
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
    // Both helpers contain the word "status" in their identifiers. Only the file with
    // all of "http", "status", and "code" may match.
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
    // "by" and "the" are stopwords, so "by the id" has one content word.
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
