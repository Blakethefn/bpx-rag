use super::super::super::*;
use crate::knowledge::sync::sync_source;
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;
use super::{count, sync};

#[test]
fn removing_a_source_drops_evidence_resolutions_analysis_and_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\nfn unused() {}\n",
    )
    .unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_references") > 0);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_resolutions") > 0);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM knowledge_analysis"), 1);
    assert!(count(&store, "SELECT COUNT(*) FROM knowledge_findings") > 0);

    store.remove_source("demo").unwrap();

    for table in [
        "knowledge_references",
        "knowledge_entries",
        "knowledge_resolutions",
        "knowledge_analysis",
        "knowledge_findings",
        "knowledge_symbols",
    ] {
        assert_eq!(
            count(&store, &format!("SELECT COUNT(*) FROM {table}")),
            0,
            "{table} still has rows"
        );
    }
}
