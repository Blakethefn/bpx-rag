use super::*;
use crate::knowledge::query::SearchWeights;
use crate::knowledge::{query, sync};
use tempfile::tempdir;

#[test]
pub(super) fn glob_filter_restricts_sync_and_search() {
    let store = make_store();
    let dir = tempdir().unwrap();

    std::fs::write(
        dir.path().join("included.md"),
        "# Included\n\nThe photosynthesis discussion is here.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("excluded.txt"),
        "This txt file about photosynthesis should not be indexed.",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("excluded.rs"),
        "// photosynthesis in rust, should not be indexed",
    )
    .unwrap();

    store
        .add_source(
            "md-only",
            dir.path().to_str().unwrap(),
            &["*.md".to_string()],
            &[],
        )
        .unwrap();
    let source = store.get_source("md-only").unwrap().unwrap();
    let r = sync::sync_source(&store, &source, None).unwrap();
    assert_eq!(r.files_added, 1, "only .md files should be synced");

    let weights = SearchWeights::default();
    let results =
        query::knowledge_search(&store, "photosynthesis", None, None, 10, &weights, 2000).unwrap();
    assert_eq!(
        results.len(),
        1,
        "search should only find the .md file content"
    );
}

#[test]
pub(super) fn include_and_exclude_scope_index_only_the_chosen_parts() {
    let store = make_store();
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::create_dir_all(dir.path().join("vendor")).unwrap();
    std::fs::write(
        dir.path().join("docs/guide.md"),
        "# Guide\n\nkestrel handbook",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("docs/draft.md"),
        "# Draft\n\nkestrel scratch notes",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("vendor/lib.md"),
        "# Vendored\n\nkestrel upstream copy",
    )
    .unwrap();

    let patterns = crate::knowledge::scope::patterns_from(
        &["docs/**".to_string()],
        &["docs/draft.md".to_string()],
    );
    store
        .add_source("scoped", dir.path().to_str().unwrap(), &patterns, &[])
        .unwrap();
    let source = store.get_source("scoped").unwrap().unwrap();
    assert_eq!(
        source.patterns, patterns,
        "scope round-trips through the store"
    );

    let r = sync::sync_source(&store, &source, None).unwrap();
    assert_eq!(r.files_added, 1, "only docs/guide.md is in scope");

    let results = query::knowledge_search(
        &store,
        "kestrel",
        None,
        None,
        10,
        &SearchWeights::default(),
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_path, "docs/guide.md");
}
