use std::collections::HashSet;
use std::path::Path;

use super::super::{push_entry, EntryObservation, ENTRY_LIBRARY, ENTRY_PROGRAM, ENTRY_TEST};
use super::manifest_entry;
use super::normalize_target;

pub(super) struct ExplicitTargets {
    pub(super) lib: bool,
    pub(super) bins: HashSet<String>,
    pub(super) tests: HashSet<String>,
    pub(super) examples: HashSet<String>,
    pub(super) benches: HashSet<String>,
}

pub(super) fn cargo_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let Some(lib) = value.get("lib") {
        let target = lib
            .get("path")
            .and_then(toml::Value::as_str)
            .unwrap_or("src/lib.rs");
        push_entry(
            &mut entries,
            manifest_entry(path, target, ENTRY_LIBRARY, "Cargo.toml lib"),
        );
    }
    push_table(
        &mut entries,
        path,
        value.get("bin"),
        "src/bin",
        ENTRY_PROGRAM,
        "Cargo.toml bin",
    );
    push_table(
        &mut entries,
        path,
        value.get("test"),
        "tests",
        ENTRY_TEST,
        "Cargo.toml test",
    );
    push_table(
        &mut entries,
        path,
        value.get("example"),
        "examples",
        ENTRY_PROGRAM,
        "Cargo.toml example",
    );
    push_table(
        &mut entries,
        path,
        value.get("bench"),
        "benches",
        ENTRY_PROGRAM,
        "Cargo.toml bench",
    );
    entries
}

pub(super) fn explicit_targets(value: &toml::Value) -> ExplicitTargets {
    let mut explicit = ExplicitTargets {
        lib: value.get("lib").is_some(),
        bins: HashSet::new(),
        tests: HashSet::new(),
        examples: HashSet::new(),
        benches: HashSet::new(),
    };
    collect_targets(value.get("bin"), "src/bin", &mut explicit.bins);
    collect_targets(value.get("test"), "tests", &mut explicit.tests);
    collect_targets(value.get("example"), "examples", &mut explicit.examples);
    collect_targets(value.get("bench"), "benches", &mut explicit.benches);
    explicit
}

fn collect_targets(table: Option<&toml::Value>, default_dir: &str, explicit: &mut HashSet<String>) {
    let Some(items) = table.and_then(toml::Value::as_array) else {
        return;
    };
    for item in items {
        if let Some(target) = target_path(item, default_dir) {
            explicit.insert(normalize_target(&target));
        }
    }
}

fn push_table(
    entries: &mut Vec<EntryObservation>,
    manifest: &Path,
    table: Option<&toml::Value>,
    default_dir: &str,
    kind: &'static str,
    reason: &'static str,
) {
    let Some(items) = table.and_then(toml::Value::as_array) else {
        return;
    };
    for item in items {
        let Some(target) = target_path(item, default_dir) else {
            continue;
        };
        push_entry(entries, manifest_entry(manifest, &target, kind, reason));
    }
}

fn target_path(item: &toml::Value, default_dir: &str) -> Option<String> {
    item.get("path")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.get("name")
                .and_then(toml::Value::as_str)
                .map(|name| format!("{default_dir}/{name}.rs"))
        })
}

