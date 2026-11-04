use super::*;
use crate::knowledge::query::{knowledge_search_report, SearchWeights};
use crate::knowledge::sync::sync_source;
use crate::knowledge::{open_in_memory, KnowledgeSourceInfo};
use rusqlite::params;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn make_store() -> KnowledgeStore {
    open_in_memory().expect("in-memory knowledge store")
}

fn register(store: &KnowledgeStore, label: &str, dir: &Path) -> KnowledgeSourceInfo {
    store
        .add_source(label, dir.to_str().unwrap(), &[], &[])
        .unwrap();
    store.get_source(label).unwrap().unwrap()
}

fn sync(store: &KnowledgeStore, label: &str) {
    let source = store.get_source(label).unwrap().unwrap();
    sync_source(store, &source, None).unwrap();
}

fn exact(store: &KnowledgeStore, name: &str) -> Vec<SymbolHit> {
    lookup_with(store, name, MatchMode::Exact, None, 20).hits
}

fn lookup_with(
    store: &KnowledgeStore,
    name: &str,
    mode: MatchMode,
    source: Option<&str>,
    limit: usize,
) -> SymbolReport {
    lookup(
        store,
        &SymbolQuery {
            name,
            mode,
            source,
            limit,
        },
    )
    .unwrap()
}

fn hit(
    label: &str,
    rel_path: &str,
    name: &str,
    kind: &str,
    container: Option<&str>,
    lines: (u32, u32),
) -> SymbolHit {
    SymbolHit {
        source_label: label.to_string(),
        rel_path: rel_path.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        container: container.map(str::to_string),
        line_start: lines.0,
        line_end: lines.1,
    }
}

fn count(store: &KnowledgeStore, sql: &str, label: &str) -> i64 {
    store
        .conn()
        .query_row(sql, params![label], |row| row.get(0))
        .unwrap()
}

#[test]
fn exact_lookup_returns_paths_and_ranges_for_each_language() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn rust_entry() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("tool.py"),
        "import os\n\ndef python_entry(path):\n    return path\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("app.ts"),
        "export function ts_entry(): number {\n  return 1;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("main.go"),
        "package main\n\nfunc GoEntry() int {\n\treturn 1\n}\n",
    )
    .unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    assert_eq!(
        exact(&store, "rust_entry"),
        vec![hit(
            "test",
            "lib.rs",
            "rust_entry",
            "function",
            None,
            (1, 3)
        )]
    );
    assert_eq!(
        exact(&store, "python_entry"),
        vec![hit(
            "test",
            "tool.py",
            "python_entry",
            "function",
            None,
            (3, 4)
        )]
    );
    assert_eq!(
        exact(&store, "ts_entry"),
        vec![hit("test", "app.ts", "ts_entry", "function", None, (1, 3))]
    );
    assert_eq!(
        exact(&store, "GoEntry"),
        vec![hit("test", "main.go", "GoEntry", "function", None, (3, 5))]
    );
}

#[test]
fn duplicate_names_in_two_files_return_both_paths_in_order() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("b")).unwrap();
    fs::create_dir_all(dir.path().join("a")).unwrap();
    fs::write(dir.path().join("b/two.rs"), "pub fn shared() -> u8 { 2 }\n").unwrap();
    fs::write(dir.path().join("a/one.rs"), "pub fn shared() -> u8 { 1 }\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    assert_eq!(
        exact(&store, "shared"),
        vec![
            hit("test", "a/one.rs", "shared", "function", None, (1, 1)),
            hit("test", "b/two.rs", "shared", "function", None, (1, 1)),
        ]
    );
}

#[test]
fn prefix_lookup_matches_names_with_the_prefix_and_exact_does_not() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("users.rs"),
        "pub fn load_user() {}\npub fn load_config() {}\npub fn unload() {}\npub fn Load_Upper() {}\n",
    )
    .unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    let prefixed = lookup_with(&store, "load_", MatchMode::Prefix, None, 20);
    let names: Vec<&str> = prefixed.hits.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, vec!["load_config", "load_user"]);
    assert!(exact(&store, "load").is_empty());
    assert!(exact(&store, "load_user").len() == 1);
    assert!(
        exact(&store, "load_upper").is_empty(),
        "lookup is case-sensitive"
    );
}

#[test]
fn source_filter_restricts_results_to_one_source() {
    let store = make_store();
    let one = tempdir().unwrap();
    let two = tempdir().unwrap();
    fs::write(one.path().join("a.rs"), "pub fn common() {}\n").unwrap();
    fs::write(two.path().join("b.rs"), "pub fn common() {}\n").unwrap();
    register(&store, "one", one.path());
    register(&store, "two", two.path());
    sync(&store, "one");
    sync(&store, "two");

    let report = lookup_with(&store, "common", MatchMode::Exact, Some("two"), 20);
    assert_eq!(
        report.hits,
        vec![hit("two", "b.rs", "common", "function", None, (1, 1))]
    );
    assert_eq!(exact(&store, "common").len(), 2);
}

#[test]
fn limit_truncates_and_reports_it() {
    let store = make_store();
    let dir = tempdir().unwrap();
    for index in 0..3 {
        fs::write(
            dir.path().join(format!("f{index}.rs")),
            "pub fn shared() {}\n",
        )
        .unwrap();
    }
    register(&store, "test", dir.path());
    sync(&store, "test");

    let report = lookup_with(&store, "shared", MatchMode::Exact, None, 2);
    assert_eq!(report.hits.len(), 2);
    assert!(report.truncated);
    let full = lookup_with(&store, "shared", MatchMode::Exact, None, 3);
    assert_eq!(full.hits.len(), 3);
    assert!(!full.truncated);
}

#[test]
fn empty_and_oversized_names_are_errors() {
    let store = make_store();
    let empty = lookup(
        &store,
        &SymbolQuery {
            name: "   ",
            mode: MatchMode::Exact,
            source: None,
            limit: 20,
        },
    );
    assert!(empty.is_err());
    let long = "a".repeat(257);
    let oversized = lookup(
        &store,
        &SymbolQuery {
            name: &long,
            mode: MatchMode::Exact,
            source: None,
            limit: 20,
        },
    );
    assert!(oversized.is_err());
}

#[test]
fn rename_on_resync_replaces_the_old_name() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn old_name() {}\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");
    assert_eq!(exact(&store, "old_name").len(), 1);

    fs::write(dir.path().join("lib.rs"), "pub fn new_name() {}\n").unwrap();
    sync(&store, "test");

    assert!(exact(&store, "old_name").is_empty());
    assert_eq!(
        exact(&store, "new_name"),
        vec![hit("test", "lib.rs", "new_name", "function", None, (1, 1))]
    );
}

#[test]
fn moved_file_drops_the_old_path_and_indexes_the_new_one() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn moved() {}\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::rename(dir.path().join("lib.rs"), dir.path().join("sub/lib.rs")).unwrap();
    sync(&store, "test");

    assert_eq!(
        exact(&store, "moved"),
        vec![hit("test", "sub/lib.rs", "moved", "function", None, (1, 1))]
    );
}

#[test]
fn removed_file_drops_its_symbols() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("keep.rs"), "pub fn keep() {}\n").unwrap();
    fs::write(dir.path().join("gone.rs"), "pub fn gone() {}\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    fs::remove_file(dir.path().join("gone.rs")).unwrap();
    sync(&store, "test");

    assert!(exact(&store, "gone").is_empty());
    assert_eq!(exact(&store, "keep").len(), 1);
    let orphans: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_symbols ks
             LEFT JOIN knowledge_files kf ON kf.id = ks.file_id
             WHERE kf.id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);
}

#[test]
fn parse_error_file_keeps_its_chunks_and_reports_no_symbols() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("broken.rs"),
        "pub fn fine() {}\nfn broken( {\n",
    )
    .unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    assert!(exact(&store, "fine").is_empty());
    let chunks: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM knowledge_chunk_meta kcm
             JOIN knowledge_files kf ON kf.id = kcm.file_id
             WHERE kf.rel_path = 'broken.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(chunks > 0, "parse error must keep the file's chunks");
    let report = lookup_with(&store, "fine", MatchMode::Exact, None, 20);
    assert!(
        report.pending.is_empty(),
        "a parse-error file is indexed, not pending"
    );
}

#[test]
fn removing_a_source_deletes_its_symbol_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn gone_with_source() {}\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM knowledge_symbols ks JOIN knowledge_sources s ON s.id = ks.source_id WHERE s.label = ?1",
            "test"
        ),
        1
    );

    store.remove_source("test").unwrap();

    let total: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_symbols", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(total, 0);
}

#[test]
fn existing_index_is_backfilled_on_sync_without_rechunking() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn legacy_entry() {}\n").unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    // Simulate a database built before symbols existed: no rows and version 0.
    store
        .conn()
        .execute_batch(
            "DELETE FROM knowledge_symbols;
             UPDATE knowledge_files SET symbol_version = 0;",
        )
        .unwrap();
    let chunk_rows_before: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };

    let pending = lookup_with(&store, "legacy_entry", MatchMode::Exact, None, 20);
    assert!(pending.hits.is_empty());
    assert_eq!(
        pending.pending,
        vec![PendingSymbols {
            source_label: "test".to_string(),
            files: 1
        }]
    );

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();
    assert_eq!(
        result.files_updated, 0,
        "backfill must not re-chunk the file"
    );
    assert_eq!(result.files_unchanged, 1);
    assert_eq!(result.chunks_indexed, 0);

    let chunk_rows_after: Vec<i64> = {
        let mut statement = store
            .conn()
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta ORDER BY chunk_rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(chunk_rows_before, chunk_rows_after);

    let after = lookup_with(&store, "legacy_entry", MatchMode::Exact, None, 20);
    assert_eq!(after.hits.len(), 1);
    assert!(after.pending.is_empty());
}

#[test]
fn default_search_does_not_read_the_symbol_index() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "/// Loads the configured user record.\npub fn load_user_record() -> u8 { 1 }\n",
    )
    .unwrap();
    register(&store, "test", dir.path());
    sync(&store, "test");

    let weights = SearchWeights::default();
    let search = |store: &KnowledgeStore| {
        knowledge_search_report(store, "user record", None, None, 10, &weights, 200)
            .unwrap()
            .results
            .iter()
            .map(|result| (result.file_path.clone(), result.lines.clone(), result.score))
            .collect::<Vec<_>>()
    };
    let before = search(&store);
    store
        .conn()
        .execute_batch("DELETE FROM knowledge_symbols")
        .unwrap();
    let after = search(&store);
    assert!(!before.is_empty());
    assert_eq!(before, after);
}
