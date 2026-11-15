use super::super::super::*;
use super::super::{exact, hit, lookup_with, sync};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn source_filter_restricts_results_to_one_source() {
    let store = make_store();
    let one = tempdir().unwrap();
    let two = tempdir().unwrap();
    fs::write(one.path().join("a.rs"), "pub fn common() {}\n").unwrap();
    fs::write(two.path().join("b.rs"), "pub fn common() {}\n").unwrap();
    add_source(&store, "one", one.path());
    add_source(&store, "two", two.path());
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
    add_source(&store, "test", dir.path());
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
