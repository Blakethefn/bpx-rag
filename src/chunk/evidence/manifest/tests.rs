use std::collections::HashSet;
use std::path::Path;

use super::catalog::TARGETS;
use super::{implicit_cargo_entries, manifest_entries};

fn indexed(paths: impl IntoIterator<Item = impl AsRef<str>>) -> HashSet<String> {
    paths
        .into_iter()
        .map(|path| path.as_ref().to_string())
        .collect()
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

#[test]
fn each_catalog_row_drives_explicit_emit_suppression_and_implicit_emit() {
    for target in TARGETS {
        let explicit_reason = target.explicit_reason();
        let implicit_reason = target.implicit_reason();
        let named = format!("{}/probe.rs", target.directory);
        let sibling = format!("{}/other.rs", target.directory);
        let manifest = format!(
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[{}]]\nname = \"probe\"\n",
            target.key
        );

        let explicit = manifest_entries(Path::new("Cargo.toml"), &manifest);
        let named_explicit: Vec<_> = explicit
            .iter()
            .filter(|entry| entry.reason == explicit_reason)
            .collect();
        assert_eq!(named_explicit.len(), 1, "{explicit:?}");
        assert_eq!(named_explicit[0].kind, target.kind);
        assert_eq!(
            named_explicit[0].target_rel_path.as_deref(),
            Some(named.as_str())
        );

        let custom = format!(
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[{}]]\nname = \"probe\"\npath = \"nested/probe.rs\"\n",
            target.key
        );
        let custom_entries = manifest_entries(Path::new("pkg/Cargo.toml"), &custom);
        assert!(
            custom_entries.iter().any(|entry| {
                entry.kind == target.kind
                    && entry.reason == explicit_reason
                    && entry.target_rel_path.as_deref() == Some("pkg/nested/probe.rs")
            }),
            "{custom_entries:?}"
        );
        assert!(
            custom_entries
                .iter()
                .all(|entry| { entry.target_rel_path.as_deref() != Some(&format!("pkg/{named}")) }),
            "{custom_entries:?}"
        );

        let implicit = implicit_cargo_entries(
            "pkg/Cargo.toml",
            &custom,
            &indexed([
                "pkg/nested/probe.rs".to_string(),
                format!("pkg/{named}"),
                format!("pkg/{sibling}"),
                "pkg/src/lib.rs".to_string(),
                "pkg/src/main.rs".to_string(),
            ]),
        );
        assert!(
            implicit
                .iter()
                .all(|entry| entry.target_rel_path.as_deref() != Some("pkg/nested/probe.rs")),
            "{implicit:?}"
        );
        assert!(
            implicit.iter().any(|entry| {
                entry.kind == target.kind
                    && entry.reason == implicit_reason
                    && entry.target_rel_path.as_deref() == Some(&format!("pkg/{sibling}"))
            }),
            "{implicit:?}"
        );
        assert!(
            implicit
                .iter()
                .any(|entry| entry.target_rel_path.as_deref() == Some("pkg/src/lib.rs")),
            "{implicit:?}"
        );

        let named_implicit = implicit_cargo_entries(
            "Cargo.toml",
            &manifest,
            &indexed([
                named.clone(),
                sibling.clone(),
                "src/lib.rs".to_string(),
                "src/main.rs".to_string(),
            ]),
        );
        assert!(
            named_implicit
                .iter()
                .all(|entry| entry.target_rel_path.as_deref() != Some(named.as_str())),
            "{named_implicit:?}"
        );
        assert!(
            named_implicit.iter().any(|entry| {
                entry.kind == target.kind
                    && entry.reason == implicit_reason
                    && entry.target_rel_path.as_deref() == Some(sibling.as_str())
            }),
            "{named_implicit:?}"
        );
        if target.key == "bin" {
            assert!(
                named_implicit.iter().any(|entry| {
                    entry.kind == target.kind
                        && entry.reason == implicit_reason
                        && entry.target_rel_path.as_deref() == Some("src/main.rs")
                }),
                "{named_implicit:?}"
            );
            let main_manifest = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"demo\"\npath = \"src/main.rs\"\n";
            let main_implicit = implicit_cargo_entries(
                "Cargo.toml",
                main_manifest,
                &indexed([sibling.clone(), "src/main.rs".to_string()]),
            );
            assert!(
                main_implicit
                    .iter()
                    .all(|entry| entry.target_rel_path.as_deref() != Some("src/main.rs")),
                "{main_implicit:?}"
            );
            assert!(
                main_implicit.iter().any(|entry| {
                    entry.kind == target.kind
                        && entry.reason == implicit_reason
                        && entry.target_rel_path.as_deref() == Some(sibling.as_str())
                }),
                "{main_implicit:?}"
            );
        }

        let mut kept_paths = vec![
            "src/lib.rs".to_string(),
            "src/main.rs".to_string(),
            sibling.clone(),
        ];
        for row in TARGETS {
            if row.key != target.key {
                kept_paths.push(format!("{}/kept.rs", row.directory));
            }
        }
        let disabled = format!(
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n{} = false\n",
            target.auto
        );
        let disabled_refs: Vec<&str> = kept_paths.iter().map(String::as_str).collect();
        let omitted = implicit_cargo_entries("Cargo.toml", &disabled, &indexed(disabled_refs));
        assert!(
            omitted
                .iter()
                .all(|entry| entry.target_rel_path.as_deref() != Some(sibling.as_str())),
            "{omitted:?}"
        );
        assert!(
            omitted
                .iter()
                .any(|entry| entry.target_rel_path.as_deref() == Some("src/lib.rs")),
            "{omitted:?}"
        );
        for row in TARGETS {
            if row.key == target.key {
                continue;
            }
            let kept = format!("{}/kept.rs", row.directory);
            assert!(
                omitted.iter().any(|entry| {
                    entry.kind == row.kind
                        && entry.reason == row.implicit_reason()
                        && entry.target_rel_path.as_deref() == Some(kept.as_str())
                }),
                "{omitted:?}"
            );
        }
        let main_present = omitted
            .iter()
            .any(|entry| entry.target_rel_path.as_deref() == Some("src/main.rs"));
        assert_eq!(main_present, target.key != "bin", "{omitted:?}");
    }
}
