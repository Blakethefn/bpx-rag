use super::super::super::KnowledgeStore;
use crate::knowledge::findings::report;
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use rusqlite::params;
use std::fs;
use tempfile::tempdir;
use super::{analysis, finding_rows, sync};

#[test]
fn an_unchanged_sync_keeps_the_fingerprint_and_the_finding_rows() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\nfn unused() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");
    let (_, generation, fingerprint, _) = analysis(&store, "demo");
    let findings = finding_rows(&store);
    assert!(!findings.is_empty());

    sync(&store, "demo");

    let (_, next_generation, next_fingerprint, _) = analysis(&store, "demo");
    assert_eq!(next_fingerprint, fingerprint);
    assert_eq!(next_generation, generation);
    assert_eq!(finding_rows(&store), findings);
}
