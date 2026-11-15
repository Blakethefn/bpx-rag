use std::fs;
use std::path::PathBuf;
use std::process::Output;
use tempfile::tempdir;

use super::harness::{run_cli, stdout};

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

pub(crate) struct Layout {
    _temp: tempfile::TempDir,
    pub(crate) root: PathBuf,
    pub(crate) project: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) cache: PathBuf,
}

pub(crate) fn layout() -> Layout {
    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    Layout {
        config: root.join("config"),
        data: root.join("data"),
        cache: root.join("cache"),
        project,
        root,
        _temp: temp,
    }
}

#[test]
fn findings_returns_complete_analysis() {
    let home = layout();
    let complete = home.root.join("complete");
    fs::create_dir_all(&complete).unwrap();
    fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();

    let added = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["add", complete.to_str().unwrap(), "--source", "ready"],
    );
    assert!(added.status.success(), "{}", stderr(&added));

    let one = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["findings"],
    );
    let one_text = stdout(&one);
    assert!(one.status.success(), "{}", stderr(&one));
    assert!(one_text.contains("source: ready"), "{one_text}");
    assert!(one_text.contains("status: complete"), "{one_text}");
    assert!(one_text.contains("analysis_version:"), "{one_text}");
    assert!(one_text.contains("fingerprint:"), "{one_text}");
    assert!(one_text.contains("findings:"), "{one_text}");
}

#[test]
fn findings_returns_incomplete_analysis() {
    let home = layout();
    let broken = home.root.join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

    let added_broken = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["add", broken.to_str().unwrap(), "--source", "partial"],
    );
    assert!(added_broken.status.success(), "{}", stderr(&added_broken));

    let incomplete = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["findings", "--source", "partial"],
    );
    let incomplete_text = stdout(&incomplete);
    assert!(incomplete.status.success(), "{}", stderr(&incomplete));
    assert!(
        incomplete_text.contains("source: partial"),
        "{incomplete_text}"
    );
    assert!(
        incomplete_text.contains("status: incomplete"),
        "{incomplete_text}"
    );
    assert!(incomplete_text.contains("broken.rs"), "{incomplete_text}");
    assert!(
        incomplete_text.contains("fingerprint:"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("possibly_unused_symbol"),
        "{incomplete_text}"
    );
    assert!(
        !incomplete_text.contains("unreachable_file"),
        "{incomplete_text}"
    );
}
