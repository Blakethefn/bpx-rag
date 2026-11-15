use super::super::*;
use super::{exact, hit, lookup_with, sync};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

mod filter;

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
    add_source(&store, "test", dir.path());
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
    add_source(&store, "test", dir.path());
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
    add_source(&store, "test", dir.path());
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
