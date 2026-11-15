use super::*;

#[test]
fn each_catalog_row_drives_explicit_emit() {
    for target in TARGETS {
        let explicit_reason = target.explicit_reason();
        let named = format!("{}/probe.rs", target.directory);
        let explicit = manifest_entries(Path::new("Cargo.toml"), &named_manifest(target.key));
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

        let custom_entries =
            manifest_entries(Path::new("pkg/Cargo.toml"), &custom_manifest(target.key));
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
    }
}
