use super::*;

#[test]
fn each_catalog_row_suppresses_explicit_and_disabled_paths() {
    for target in TARGETS {
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
            implicit
                .iter()
                .all(|entry| entry.target_rel_path.as_deref() != Some("pkg/nested/probe.rs")),
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
        if target.key == "bin" {
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
                .all(|entry| entry.target_rel_path.as_deref() != Some(sibling.as_str())),
            "{omitted:?}"
        );
    }
}
