use std::collections::HashSet;
use std::path::Path;

use super::catalog::TARGETS;
use super::{implicit_cargo_entries, manifest_entries};

mod emit;
mod implicit;
mod suppress;

fn indexed(paths: impl IntoIterator<Item = impl AsRef<str>>) -> HashSet<String> {
    paths
        .into_iter()
        .map(|path| path.as_ref().to_string())
        .collect()
}

fn named_manifest(key: &str) -> String {
    format!("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[{key}]]\nname = \"probe\"\n")
}

fn custom_manifest(key: &str) -> String {
    format!(
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[{key}]]\nname = \"probe\"\npath = \"nested/probe.rs\"\n"
    )
}

fn disabled_manifest(auto: &str) -> String {
    format!("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n{auto} = false\n")
}

fn kept_paths(target_key: &str, sibling: &str) -> Vec<String> {
    let mut paths = vec![
        "src/lib.rs".to_string(),
        "src/main.rs".to_string(),
        sibling.to_string(),
    ];
    for row in TARGETS {
        if row.key != target_key {
            paths.push(format!("{}/kept.rs", row.directory));
        }
    }
    paths
}

#[test]
fn catalog_is_the_cargo_auto_target_table() {
    let rows: Vec<_> = TARGETS
        .iter()
        .map(|target| (target.key, target.directory, target.kind, target.auto))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("bin", "src/bin", "program", "autobins"),
            ("test", "tests", "test", "autotests"),
            ("example", "examples", "program", "autoexamples"),
            ("bench", "benches", "program", "autobenches"),
        ]
    );
}
