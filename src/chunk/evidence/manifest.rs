//! Entry paths named by a manifest this repository already recognizes.
//!
//! A manifest the extractor cannot parse as source is still evidence for these rows.

use std::path::{Component, Path};

use super::{push_entry, EntryObservation, ENTRY_LIBRARY, ENTRY_PROGRAM, ENTRY_TEST};

pub(super) fn manifest_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    match name {
        "Cargo.toml" => cargo_entries(path, content),
        "package.json" => package_json_entries(path, content),
        _ => Vec::new(),
    }
}

fn cargo_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
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
    if let Some(bins) = value.get("bin").and_then(toml::Value::as_array) {
        for bin in bins {
            let target = bin
                .get("path")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    bin.get("name")
                        .and_then(toml::Value::as_str)
                        .map(|name| format!("src/bin/{name}.rs"))
                });
            if let Some(target) = target {
                push_entry(
                    &mut entries,
                    manifest_entry(path, &target, ENTRY_PROGRAM, "Cargo.toml bin"),
                );
            }
        }
    }
    if let Some(tests) = value.get("test").and_then(toml::Value::as_array) {
        for test in tests {
            let target = test
                .get("path")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    test.get("name")
                        .and_then(toml::Value::as_str)
                        .map(|name| format!("tests/{name}.rs"))
                });
            if let Some(target) = target {
                push_entry(
                    &mut entries,
                    manifest_entry(path, &target, ENTRY_TEST, "Cargo.toml test"),
                );
            }
        }
    }
    entries
}

fn package_json_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let Some(main) = value.get("main").and_then(serde_json::Value::as_str) {
        push_entry(
            &mut entries,
            manifest_entry(path, main, ENTRY_PROGRAM, "package.json main"),
        );
    }
    if let Some(bins) = value.get("bin").and_then(serde_json::Value::as_object) {
        for target in bins.values().filter_map(serde_json::Value::as_str) {
            push_entry(
                &mut entries,
                manifest_entry(path, target, ENTRY_PROGRAM, "package.json bin"),
            );
        }
    }
    entries
}

fn manifest_entry(
    manifest: &Path,
    target: &str,
    kind: &'static str,
    reason: &'static str,
) -> EntryObservation {
    let parent = manifest.parent().unwrap_or(Path::new(""));
    EntryObservation {
        target_rel_path: Some(normalize_rel(&parent.join(target))),
        kind,
        reason,
    }
}

fn normalize_rel(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}
