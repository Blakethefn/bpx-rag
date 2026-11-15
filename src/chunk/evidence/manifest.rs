use std::path::{Component, Path};

use super::{push_entry, EntryObservation, ENTRY_PROGRAM};

mod catalog;
mod explicit;
mod implicit;

#[cfg(test)]
mod tests;

pub(crate) use implicit::implicit_cargo_entries;

pub(super) fn manifest_entries(path: &Path, content: &str) -> Vec<EntryObservation> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    match name {
        "Cargo.toml" => explicit::cargo_entries(path, content),
        "package.json" => package_json_entries(path, content),
        _ => Vec::new(),
    }
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
    reason: &str,
) -> EntryObservation {
    let parent = manifest.parent().unwrap_or(Path::new(""));
    EntryObservation {
        target_rel_path: Some(normalize_rel(&parent.join(target))),
        kind,
        reason: reason.to_string(),
    }
}

fn normalize_target(target: &str) -> String {
    normalize_rel(Path::new(target))
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
