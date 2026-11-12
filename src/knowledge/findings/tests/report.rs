use super::super::report;
use super::{analysis, finding_rows, sync};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn the_reader_returns_the_analysis_row_and_hides_findings_that_do_not_match_it() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let text = report(&store, None).unwrap();
    assert!(
        text.starts_with("source: demo\nstatus: complete\n"),
        "{text}"
    );
    assert!(text.contains("fingerprint: "), "{text}");
    assert!(text.contains("possibly_unused_symbol"), "{text}");

    store
        .conn()
        .execute("UPDATE knowledge_findings SET fingerprint = 'stale'", [])
        .unwrap();
    let hidden = report(&store, Some("demo")).unwrap();
    assert!(hidden.contains("status: complete"), "{hidden}");
    assert!(
        hidden.contains("findings:\nnone") || hidden.ends_with("findings:\nnone"),
        "{hidden}"
    );
    assert!(!hidden.contains("possibly_unused_symbol"), "{hidden}");
}

#[test]
fn a_source_with_no_analysis_row_does_not_invent_a_fingerprint() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    add_source(&store, "demo", dir.path());

    let text = report(&store, None).unwrap();
    assert_eq!(text, "source: demo\nno analysis stored\n");
    assert!(!text.contains("fingerprint"));
}

#[test]
fn several_sources_and_no_label_names_the_labels() {
    let store = make_store();
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    fs::write(first.path().join("a.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(second.path().join("b.rs"), "pub fn beta() {}\n").unwrap();
    add_source(&store, "alpha", first.path());
    add_source(&store, "beta", second.path());

    let error = report(&store, None).unwrap_err().to_string();
    assert!(error.contains("alpha"), "{error}");
    assert!(error.contains("beta"), "{error}");
    assert!(error.contains("source label is required"), "{error}");
}

#[test]
fn every_rust_file_failing_to_parse_is_incomplete_and_has_no_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "fn broken( {\n").unwrap();
    fs::write(dir.path().join("b.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "incomplete");
    assert!(gaps.contains("a.rs:unsealed"), "{gaps}");
    assert!(gaps.contains("b.rs:unsealed"), "{gaps}");
    assert!(finding_rows(&store).is_empty());
    let text = report(&store, Some("demo")).unwrap();
    assert!(!text.contains("findings:"), "{text}");
}

#[test]
fn tampering_a_partial_fingerprint_hides_finding_lines_and_keeps_the_header() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "mod util;\nmod other;\nfn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/util.rs"), "fn hidden() {}\n").unwrap();
    fs::write(dir.path().join("src/other.rs"), "fn broken( {\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    store
        .conn()
        .execute("UPDATE knowledge_analysis SET fingerprint = 'tampered'", [])
        .unwrap();
    let hidden = report(&store, Some("demo")).unwrap();
    assert!(
        hidden.starts_with("source: demo\nstatus: partial\n"),
        "{hidden}"
    );
    assert!(hidden.contains("fingerprint: tampered"), "{hidden}");
    assert!(hidden.contains("src/other.rs:unsealed"), "{hidden}");
    assert!(
        hidden.contains("findings:\nnone") || hidden.ends_with("findings:\nnone"),
        "{hidden}"
    );
    assert!(!hidden.contains("possibly_unused_symbol"), "{hidden}");
}

#[test]
fn a_markdown_only_source_stays_complete_and_has_no_findings() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("README.md"), "# Notes\n").unwrap();
    fs::write(dir.path().join("docs/guide.md"), "A guide.\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "complete");
    assert_eq!(
        gaps,
        "README.md:unsupported\ndocs/guide.md:unsupported\nno_entry_points"
    );
    assert!(
        finding_rows(&store).is_empty(),
        "{:?}",
        finding_rows(&store)
    );
}
