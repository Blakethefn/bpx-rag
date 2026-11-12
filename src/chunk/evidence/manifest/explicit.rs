use std::collections::HashSet;
use std::path::Path;

use super::super::{push_entry, EntryObservation, ENTRY_LIBRARY};
use super::catalog;
use super::manifest_entry;
use super::normalize_target;

pub(super) struct ExplicitTargets {
    pub(super) lib: bool,
    pub(super) suppressed: Vec<HashSet<String>>,
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
    for target in catalog::TARGETS {
        let reason = target.explicit_reason();
        push_table(
            &mut entries,
            path,
            value.get(target.key),
            target.directory,
            target.kind,
            &reason,
        );
    }
    entries
}

pub(super) fn explicit_targets(value: &toml::Value) -> ExplicitTargets {
    let suppressed = catalog::TARGETS
        .iter()
        .map(|target| {
            let mut paths = HashSet::new();
            collect_targets(value.get(target.key), target.directory, &mut paths);
            paths
        })
        .collect();
    ExplicitTargets {
        lib: value.get("lib").is_some(),
        suppressed,
    }
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
    reason: &str,
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
