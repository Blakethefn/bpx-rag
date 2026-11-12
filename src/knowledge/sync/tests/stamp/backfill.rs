use super::super::super::*;
use super::sync;
use crate::support::{add_source, make_store};
use rusqlite::params;
use std::fs;
use tempfile::tempdir;

fn analysis(store: &KnowledgeStore, label: &str) -> (i64, String) {
    store
        .conn()
        .query_row(
            "SELECT a.evidence_generation, a.fingerprint
             FROM knowledge_analysis a
             JOIN knowledge_sources s ON s.id = a.source_id
             WHERE s.label = ?1",
            params![label],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn versions(store: &KnowledgeStore, rel_path: &str) -> (i64, i64) {
    store
        .conn()
        .query_row(
            "SELECT symbol_version, reference_version FROM knowledge_files WHERE rel_path = ?1",
            params![rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn visibilities(store: &KnowledgeStore, rel_path: &str) -> Vec<(String, Option<String>)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT ks.name, ks.visibility FROM knowledge_symbols ks
             JOIN knowledge_files kf ON kf.id = ks.file_id
             WHERE kf.rel_path = ?1 ORDER BY ks.name",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn reference_rows(store: &KnowledgeStore, rel_path: &str) -> Vec<(String, String, Option<String>)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT kr.kind, kr.specifier, kr.qualifier FROM knowledge_references kr
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = ?1 ORDER BY kr.kind, kr.specifier, kr.qualifier",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn an_unchanged_tree_backfills_visibility_and_references_once() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "#[path = \"other.rs\"]\nmod util;\nuse foo::bar as baz;\nuse foo::*;\nfn hidden() {}\npub(crate) fn shared() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("app.py"), "def load():\n    return 1\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");
    let (generation, _) = analysis(&store, "demo");

    store
        .conn()
        .execute(
            "UPDATE knowledge_files SET symbol_version = 1, reference_version = 1",
            [],
        )
        .unwrap();
    sync(&store, "demo");

    assert_eq!(
        versions(&store, "lib.rs"),
        (
            crate::knowledge::symbols::SYMBOL_INDEX_VERSION,
            crate::knowledge::references::REFERENCE_INDEX_VERSION,
        )
    );
    assert_eq!(
        versions(&store, "app.py"),
        (
            crate::knowledge::symbols::SYMBOL_INDEX_VERSION,
            crate::knowledge::references::REFERENCE_INDEX_VERSION,
        )
    );
    assert_eq!(
        visibilities(&store, "lib.rs"),
        vec![
            ("hidden".to_string(), Some("private".to_string())),
            ("shared".to_string(), Some("pub(crate)".to_string())),
            ("util".to_string(), Some("private".to_string())),
        ]
    );
    assert_eq!(
        visibilities(&store, "app.py"),
        vec![("load".to_string(), None)]
    );
    assert_eq!(
        reference_rows(&store, "lib.rs"),
        vec![
            (
                "import".to_string(),
                "*".to_string(),
                Some("foo".to_string())
            ),
            (
                "import".to_string(),
                "baz".to_string(),
                Some("foo::bar".to_string())
            ),
            (
                "module".to_string(),
                "util".to_string(),
                Some("other.rs".to_string())
            ),
        ]
    );
    let (backfilled_generation, backfilled_fingerprint) = analysis(&store, "demo");
    assert!(
        backfilled_generation > generation,
        "the version rewind must rebuild analysis once"
    );

    sync(&store, "demo");

    let (next_generation, next_fingerprint) = analysis(&store, "demo");
    assert_eq!(next_generation, backfilled_generation);
    assert_eq!(next_fingerprint, backfilled_fingerprint);
}
