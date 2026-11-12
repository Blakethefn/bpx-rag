use std::collections::HashSet;
use std::path::{Component, Path};

use super::{push_entry, EntryObservation, ENTRY_LIBRARY, ENTRY_PROGRAM};

mod catalog;
mod explicit;

#[cfg(test)]
mod tests;

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

pub(crate) fn implicit_cargo_entries(
    manifest_rel: &str,
    content: &str,
    indexed: &HashSet<String>,
) -> Vec<EntryObservation> {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return Vec::new();
    };
    let Some(package) = value.get("package") else {
        return Vec::new();
    };
    let explicit = explicit::explicit_targets(&value);
    let manifest = Path::new(manifest_rel);
    let mut entries = Vec::new();
    if !explicit.lib {
        push_indexed(
            &mut entries,
            manifest,
            indexed,
            "src/lib.rs",
            ENTRY_LIBRARY,
            "Cargo.toml implicit lib",
            &HashSet::new(),
        );
    }
    debug_assert_eq!(catalog::TARGETS.len(), explicit.suppressed.len());
    for (target, suppressed) in catalog::TARGETS.iter().zip(&explicit.suppressed) {
        if !auto_enabled(package, target.auto) {
            continue;
        }
        let reason = target.implicit_reason();
        if target.key == "bin" {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                "src/main.rs",
                target.kind,
                &reason,
                suppressed,
            );
        }
        for relative in indexed_direct(&explicit_prefix(manifest_rel), indexed, target.directory) {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                &relative,
                target.kind,
                &reason,
                suppressed,
            );
        }
    }
    entries
}

fn auto_enabled(package: &toml::Value, key: &str) -> bool {
    package.get(key).and_then(toml::Value::as_bool) != Some(false)
}

fn push_indexed(
    entries: &mut Vec<EntryObservation>,
    manifest: &Path,
    indexed: &HashSet<String>,
    relative: &str,
    kind: &'static str,
    reason: &str,
    explicit: &HashSet<String>,
) {
    if explicit.contains(&normalize_target(relative)) {
        return;
    }
    let source_rel = source_join(&explicit_prefix(manifest.to_str().unwrap_or("")), relative);
    if !indexed.contains(&source_rel) {
        return;
    }
    push_entry(entries, manifest_entry(manifest, relative, kind, reason));
}

fn indexed_direct(prefix: &str, indexed: &HashSet<String>, dir: &str) -> Vec<String> {
    let root = source_join(prefix, dir);
    let mut relatives = Vec::new();
    let marker = format!("{root}/");
    for path in indexed {
        let Some(name) = path.strip_prefix(&marker) else {
            continue;
        };
        if name.contains('/') || !name.ends_with(".rs") || name == ".rs" {
            continue;
        }
        relatives.push(format!("{dir}/{name}"));
    }
    relatives.sort();
    relatives.dedup();
    relatives
}

fn explicit_prefix(manifest_rel: &str) -> String {
    match manifest_rel.rfind('/') {
        Some(index) => manifest_rel[..index].to_string(),
        None => String::new(),
    }
}

fn source_join(prefix: &str, relative: &str) -> String {
    let combined = if prefix.is_empty() {
        relative.to_string()
    } else {
        format!("{prefix}/{relative}")
    };
    normalize_rel(Path::new(&combined))
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
