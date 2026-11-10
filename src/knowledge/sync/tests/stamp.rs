use super::super::*;
use rusqlite::params;
use std::fs;
use tempfile::tempdir;

fn make_store() -> KnowledgeStore {
    crate::knowledge::open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, label: &str, dir: &Path) {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
}

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn status_of(store: &KnowledgeStore, rel_path: &str) -> (String, String) {
    store
        .conn()
        .query_row(
            "SELECT symbol_status, reference_status FROM knowledge_files WHERE rel_path = ?1",
            params![rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn names(store: &KnowledgeStore, rel_path: &str) -> Vec<String> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT ks.name FROM knowledge_symbols ks
             JOIN knowledge_files kf ON kf.id = ks.file_id
             WHERE kf.rel_path = ?1 ORDER BY ks.name",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn reference_specifiers(store: &KnowledgeStore, rel_path: &str) -> Vec<(String, String)> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT kr.kind, kr.specifier FROM knowledge_references kr
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = ?1 ORDER BY kr.kind, kr.specifier",
        )
        .unwrap();
    statement
        .query_map(params![rel_path], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn editing_a_file_replaces_its_symbols_references_and_status() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() { other(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    assert_eq!(names(&store, "lib.rs"), vec!["alpha".to_string()]);
    assert_eq!(
        reference_specifiers(&store, "lib.rs"),
        vec![("name".to_string(), "other".to_string())]
    );
    assert_eq!(
        status_of(&store, "lib.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    let previous_symbol: i64 = store
        .conn()
        .query_row("SELECT id FROM knowledge_symbols", [], |row| row.get(0))
        .unwrap();

    fs::write(dir.path().join("lib.rs"), "pub fn beta() { fresh(); }\n").unwrap();
    sync(&store, "demo");

    assert_eq!(names(&store, "lib.rs"), vec!["beta".to_string()]);
    assert_eq!(
        reference_specifiers(&store, "lib.rs"),
        vec![("name".to_string(), "fresh".to_string())]
    );
    assert_eq!(
        status_of(&store, "lib.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    let still_there: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_symbols WHERE id = ?1",
            params![previous_symbol],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still_there, 0, "the previous symbol row must be gone");
}

#[test]
fn a_partial_sync_does_not_rebuild_another_files_references_from_disk() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "mod b;\npub fn caller() { kept(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn kept() { other(); }\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");
    let before = reference_specifiers(&store, "b.rs");
    assert!(
        before.iter().any(|(_, specifier)| specifier == "other"),
        "{before:?}"
    );

    fs::write(
        dir.path().join("b.rs"),
        "pub fn kept() { other(); }\npub fn added() { fresh_name(); }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("a.rs"),
        "mod b;\npub fn caller() { kept(); } // edited\n",
    )
    .unwrap();
    let source = store.get_source("demo").unwrap().unwrap();
    sync_paths(&store, &source, None, &["a.rs".to_string()]).unwrap();

    assert_eq!(reference_specifiers(&store, "b.rs"), before);
    assert!(!names(&store, "b.rs").contains(&"added".to_string()));
    let other_status: String = store
        .conn()
        .query_row(
            "SELECT res.status FROM knowledge_resolutions res
             JOIN knowledge_references kr ON kr.id = res.reference_id
             JOIN knowledge_files kf ON kf.id = kr.file_id
             WHERE kf.rel_path = 'b.rs' AND kr.specifier = 'other'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(other_status, "unresolved");
}

#[test]
fn a_file_that_declares_nothing_stores_complete_and_no_symbol_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("quiet.rs"), "// no declarations\n").unwrap();
    register(&store, "demo", dir.path());
    sync(&store, "demo");

    assert_eq!(
        status_of(&store, "quiet.rs"),
        ("complete".to_string(), "complete".to_string())
    );
    assert!(names(&store, "quiet.rs").is_empty());
    let stamped: String = store
        .conn()
        .query_row(
            "SELECT CASE WHEN symbols_hash = content_hash AND references_hash = content_hash
                         THEN 'stamped' ELSE 'unstamped' END
             FROM knowledge_files WHERE rel_path = 'quiet.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stamped, "stamped");
}

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
    register(&store, "demo", dir.path());
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
