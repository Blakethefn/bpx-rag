//! Entry paths named by a manifest this repository already recognizes.
//!
//! A manifest the extractor cannot parse as source is still evidence for these rows.
//! Explicit targets are written when the manifest is chunked. Implicit Cargo roots are chosen
//! later from the paths the sync has indexed, so a file added after that chunk still becomes
//! a root.

use std::collections::HashSet;
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

/// Implicit layout roots whose paths are in `indexed`. `indexed` holds source-relative paths.
/// A `[lib]` table suppresses the implicit library. An explicit target suppresses the implicit
/// entry for that same path. `autobins`, `autoexamples`, `autotests`, and `autobenches` set
/// false omit that kind. A manifest without `[package]` has no implicit roots.
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
    let explicit = explicit_targets(&value);
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
    if auto_enabled(package, "autobins") {
        push_indexed(
            &mut entries,
            manifest,
            indexed,
            "src/main.rs",
            ENTRY_PROGRAM,
            "Cargo.toml implicit bin",
            &explicit.bins,
        );
        for relative in indexed_direct(&explicit_prefix(manifest_rel), indexed, "src/bin") {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                &relative,
                ENTRY_PROGRAM,
                "Cargo.toml implicit bin",
                &explicit.bins,
            );
        }
    }
    if auto_enabled(package, "autoexamples") {
        for relative in indexed_direct(&explicit_prefix(manifest_rel), indexed, "examples") {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                &relative,
                ENTRY_PROGRAM,
                "Cargo.toml implicit example",
                &explicit.examples,
            );
        }
    }
    if auto_enabled(package, "autotests") {
        for relative in indexed_direct(&explicit_prefix(manifest_rel), indexed, "tests") {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                &relative,
                ENTRY_TEST,
                "Cargo.toml implicit test",
                &explicit.tests,
            );
        }
    }
    if auto_enabled(package, "autobenches") {
        for relative in indexed_direct(&explicit_prefix(manifest_rel), indexed, "benches") {
            push_indexed(
                &mut entries,
                manifest,
                indexed,
                &relative,
                ENTRY_PROGRAM,
                "Cargo.toml implicit bench",
                &explicit.benches,
            );
        }
    }
    entries
}

struct ExplicitTargets {
    lib: bool,
    bins: HashSet<String>,
    tests: HashSet<String>,
    examples: HashSet<String>,
    benches: HashSet<String>,
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

fn explicit_targets(value: &toml::Value) -> ExplicitTargets {
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

fn auto_enabled(package: &toml::Value, key: &str) -> bool {
    package.get(key).and_then(toml::Value::as_bool) != Some(false)
}

fn push_indexed(
    entries: &mut Vec<EntryObservation>,
    manifest: &Path,
    indexed: &HashSet<String>,
    relative: &str,
    kind: &'static str,
    reason: &'static str,
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

/// Package-relative `dir/*.rs` paths that are indexed directly under that directory.
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
    reason: &'static str,
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
