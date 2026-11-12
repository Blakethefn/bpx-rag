use super::super::lexical::knowledge_identifier_search;
use super::super::*;
use super::fuzzy::bm25_hits;
use super::make_store;
use super::rules::file_paths_of_ranked;

// ── identifier layer ─────────────────────────────────────────────────────

/// Index `files` through the real sync path, so tests see the same rows that
/// indexing writes. The source is labelled `code`.
pub(super) fn indexed_store(files: &[(&str, &str)]) -> (KnowledgeStore, tempfile::TempDir) {
    let store = make_store();
    let dir = tempfile::tempdir().unwrap();
    for (name, content) in files {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    store
        .add_source("code", &dir.path().to_string_lossy(), &[], &[])
        .unwrap();
    let source = store.get_source("code").unwrap().unwrap();
    crate::knowledge::sync::sync_source(&store, &source, None).unwrap();
    (store, dir)
}

fn file_paths(results: &[KnowledgeSearchResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect()
}

pub(super) const GET_USER_BY_ID: &str =
    "export function getUserById(id) {\n  return users.get(id);\n}\n";

#[test]
fn identifier_layer_matches_a_natural_language_query_to_a_camel_case_identifier() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let hits = knowledge_identifier_search(store.conn(), "get user by id", 10, "", &[]).unwrap();

    assert_eq!(file_paths_of_ranked(&hits), ["users.ts"]);
}

#[test]
fn search_finds_a_camel_case_identifier_from_split_words_when_no_other_layer_matches() {
    // Nothing in this chunk tokenizes to "get", "user", "by", or "id", so only
    // the identifier layer can match the query.
    let (store, _dir) = indexed_store(&[(
        "users.ts",
        "export function getUserById(lookup) { return cache.fetch(lookup); }\n",
    )]);

    let results = knowledge_search(
        &store,
        "get user by id",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert_eq!(file_paths(&results), ["users.ts"]);
}

#[test]
fn exact_identifier_query_still_returns_the_exact_chunk_first() {
    let (store, _dir) = indexed_store(&[
        ("users.ts", GET_USER_BY_ID),
        (
            "email.ts",
            "export function getUserByEmail(address) { return users.get(address); }\n",
        ),
    ]);

    let results = knowledge_search(
        &store,
        "getUserById",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();

    assert_eq!(results[0].file_path, "users.ts");
}

#[test]
fn indexed_content_keeps_the_original_identifier_and_stores_split_words_apart() {
    let (store, _dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);

    let content: String = store
        .conn()
        .query_row(
            "SELECT kc.content FROM knowledge_chunks kc
             JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = kc.rowid
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let words: String = store
        .conn()
        .query_row(
            "SELECT words FROM knowledge_identifiers LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        content.contains("export function getUserById(id) {\n  return users.get(id);\n}"),
        "content was rewritten: {content:?}"
    );
    assert_eq!(words, "get user by id");
}

#[test]
fn reindexing_a_file_removes_the_identifier_words_of_its_old_content() {
    let (store, dir) = indexed_store(&[("users.ts", GET_USER_BY_ID)]);
    std::fs::write(
        dir.path().join("users.ts"),
        "export function listActiveUsers() { return []; }\n",
    )
    .unwrap();
    let source = store.get_source("code").unwrap().unwrap();
    crate::knowledge::sync::sync_source(&store, &source, None).unwrap();

    // Query terms are OR-combined, so a search cannot prove a row is gone. Check
    // the rows directly: only the new chunk's words may remain.
    let mut stmt = store
        .conn()
        .prepare("SELECT words FROM knowledge_identifiers")
        .unwrap();
    let words: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(words, ["list active users"]);

    let current =
        knowledge_identifier_search(store.conn(), "list active users", 10, "", &[]).unwrap();
    assert_eq!(file_paths_of_ranked(&current), ["users.ts"]);
}

#[test]
fn snake_and_kebab_query_terms_match_their_indexed_word_tokens() {
    // Characterization of D12: `sanitize_fts_terms` keeps `_` and `-`, and the
    // quoted phrase must match the tokens that unicode61 makes from them.
    let (store, _dir) = indexed_store(&[
        ("retry.py", "def get_user(identifier):\n    return None\n"),
        (
            "deploy.md",
            "Run the blue-green-deploy script before the release.\n",
        ),
    ]);

    let paths_for = |query: &str| {
        bm25_hits(&store, query)
            .into_iter()
            .map(|(_, path)| path)
            .collect::<Vec<_>>()
    };

    assert_eq!(paths_for("get_user"), ["retry.py"]);
    assert_eq!(paths_for("get-user"), ["retry.py"]);
    assert_eq!(paths_for("blue-green-deploy"), ["deploy.md"]);
}
