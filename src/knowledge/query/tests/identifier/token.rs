use super::super::fuzzy::bm25_hits;
use super::indexed_store;

#[test]
fn snake_and_kebab_query_terms_match_their_indexed_word_tokens() {
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
