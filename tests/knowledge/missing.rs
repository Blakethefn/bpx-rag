use std::fs;

use super::findings::{layout, stderr};
use super::harness::{run_cli, stdout};

#[test]
fn findings_rejects_a_missing_label() {
    let home = layout();
    let complete = home.root.join("complete");
    let broken = home.root.join("broken");
    fs::create_dir_all(&complete).unwrap();
    fs::create_dir_all(&broken).unwrap();
    fs::write(complete.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(broken.join("broken.rs"), "pub fn fine() {}\nfn broken( {\n").unwrap();

    let added = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["add", complete.to_str().unwrap(), "--source", "ready"],
    );
    assert!(added.status.success(), "{}", stderr(&added));
    let added_broken = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["add", broken.to_str().unwrap(), "--source", "partial"],
    );
    assert!(added_broken.status.success(), "{}", stderr(&added_broken));

    let missing = run_cli(
        &home.project,
        &home.config,
        &home.data,
        &home.cache,
        &["findings"],
    );
    let missing_text = stderr(&missing);
    assert!(!missing.status.success(), "{}", stdout(&missing));
    assert!(missing_text.contains("ready"), "{missing_text}");
    assert!(missing_text.contains("partial"), "{missing_text}");
}
