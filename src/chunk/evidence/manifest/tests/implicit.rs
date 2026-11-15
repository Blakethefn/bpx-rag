use super::*;

#[test]
fn each_catalog_row_emits_implicit_targets() {
    for target in TARGETS {
        let implicit_reason = target.implicit_reason();
        let named = format!("{}/probe.rs", target.directory);
        let sibling = format!("{}/other.rs", target.directory);
        let manifest = named_manifest(target.key);
        let custom = custom_manifest(target.key);

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
                named,
                sibling.clone(),
                "src/lib.rs".to_string(),
                "src/main.rs".to_string(),
            ]),
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
                main_implicit.iter().any(|entry| {
                    entry.kind == target.kind
                        && entry.reason == implicit_reason
                        && entry.target_rel_path.as_deref() == Some(sibling.as_str())
                }),
                "{main_implicit:?}"
            );
        }

        let paths = kept_paths(target.key, &sibling);
        let disabled_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let omitted = implicit_cargo_entries(
            "Cargo.toml",
            &disabled_manifest(target.auto),
            &indexed(disabled_refs),
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
