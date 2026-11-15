use crate::knowledge::query::{self, KnowledgeSearchResult, SearchWeights};
use crate::knowledge::{KnowledgeStore, enrichment, sync};
use crate::support::{make_store, make_store_with_embedder};
use std::path::Path;
use tempfile::tempdir;

mod lifecycle;

fn search(store: &KnowledgeStore, query_text: &str, filter: &str) -> Vec<KnowledgeSearchResult> {
    query::knowledge_search(
        store,
        query_text,
        Some(filter),
        None,
        20,
        &SearchWeights::default(),
        2000,
    )
    .unwrap()
}

fn add_frontmatter_source(store: &KnowledgeStore, label: &str, root: &Path) {
    let enrichments = vec!["frontmatter".to_string()];
    store
        .add_source(label, root.to_str().unwrap(), &[], &enrichments)
        .unwrap();
    let source = store.get_source(label).unwrap().unwrap();
    let enrich = enrichment::build_enrichment_fn(&enrichments, root);
    let enrich_ref = enrich
        .as_ref()
        .map(|f| f.as_ref() as &dyn Fn(&str, &Path) -> serde_json::Value);
    sync::sync_source(store, &source, enrich_ref).unwrap();
}

#[test]
fn directory_filter_matches_subtree_without_prefix_collisions() {
    let store = make_store();
    let root = tempdir().unwrap();
    for relative in [
        "docs/guide.md",
        "docs/api/reference.md",
        "docs-old/guide.md",
    ] {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# Stargazer\n\nstargazer navigation notes.").unwrap();
    }
    store
        .add_source("repo", root.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("repo").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let results = search(&store, "stargazer", "dir:docs");
    let mut paths = results
        .iter()
        .map(|result| result.file_path.as_str())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    assert_eq!(paths, vec!["docs/api/reference.md", "docs/guide.md"]);
}

#[test]
fn file_filter_matches_only_the_exact_relative_path() {
    let store = make_store();
    let root = tempdir().unwrap();
    for relative in [
        "docs/guide.md",
        "docs/guide.md.extra",
        "docs/user guide.md",
        "other/guide.md",
    ] {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# Polaris\n\npolaris field notes.").unwrap();
    }
    store
        .add_source("repo", root.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("repo").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let results = search(&store, "polaris", "file:docs/guide.md");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_path, "docs/guide.md");

    let spaced = search(&store, "polaris", "file:\"docs/user guide.md\"");
    assert_eq!(spaced.len(), 1);
    assert_eq!(spaced[0].file_path, "docs/user guide.md");
}

#[test]
fn file_filter_keeps_semantic_retrieval_enabled() {
    let store = make_store_with_embedder();
    let root = tempdir().unwrap();
    let path = root.path().join("docs/astronomy.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "# Stellar motion\n\nOrbiting bodies trace curved paths.",
    )
    .unwrap();
    store
        .add_source("repo", root.path().to_str().unwrap(), &[], &[])
        .unwrap();
    let source = store.get_source("repo").unwrap().unwrap();
    sync::sync_source(&store, &source, None).unwrap();

    let weights = SearchWeights {
        keyword_weight: 0.0,
        vector_weight: 1.0,
    };
    let results = query::knowledge_search(
        &store,
        "celestial mechanics",
        Some("file:docs/astronomy.md"),
        None,
        10,
        &weights,
        2000,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_path, "docs/astronomy.md");
}

#[test]
fn source_metadata_and_directory_filters_intersect() {
    let store = make_store();
    let alpha = tempdir().unwrap();
    let beta = tempdir().unwrap();
    for (root, files) in [
        (
            alpha.path(),
            [
                ("docs/task.md", "task"),
                ("docs/note.md", "note"),
                ("docs-old/task.md", "task"),
            ],
        ),
        (
            beta.path(),
            [
                ("docs/task.md", "task"),
                ("docs/note.md", "note"),
                ("docs-old/task.md", "task"),
            ],
        ),
    ] {
        for (relative, kind) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                path,
                format!("---\ntype: {kind}\n---\n# Quasar\n\nquasar system documentation."),
            )
            .unwrap();
        }
    }
    add_frontmatter_source(&store, "alpha", alpha.path());
    add_frontmatter_source(&store, "beta", beta.path());

    let results = search(&store, "quasar", "source:alpha type:task dir:docs");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_label, "alpha");
    assert_eq!(results[0].file_path, "docs/task.md");
    assert_eq!(results[0].metadata["frontmatter"]["type"], "task");
}
