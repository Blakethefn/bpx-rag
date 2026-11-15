use super::super::{analysis, entries, file_reason, finding_rows, has_entry, sync, unused};
use crate::support::{add_source, make_store};
use std::fs;
use tempfile::tempdir;

#[test]
fn pub_super_is_reported_and_pub_in_paths_follow_their_scope() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "mod util;\nmod other;\n").unwrap();
    fs::write(
        dir.path().join("src/util.rs"),
        "pub(super) fn parent_scoped() {}\npub(in super::other) fn sibling_scoped() {}\npub(in crate::util) fn crate_scoped() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/other.rs"), "// sealed scope\n").unwrap();
    fs::write(dir.path().join("src/broken.rs"), "fn broken( {\n").unwrap();
    add_source(&store, "demo", dir.path());
    sync(&store, "demo");

    let (status, _, _, gaps) = analysis(&store, "demo");
    assert_eq!(status, "partial");
    assert_eq!(
        gaps,
        "Cargo.toml:unsupported\nsrc/broken.rs:parse_error\nsrc/broken.rs:unsealed"
    );
    let findings = finding_rows(&store);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        unused(&findings, "src/util.rs", "parent_scoped"),
        "{findings:?}"
    );
    assert!(
        unused(&findings, "src/util.rs", "sibling_scoped"),
        "{findings:?}"
    );
    assert!(
        !unused(&findings, "src/util.rs", "crate_scoped"),
        "{findings:?}"
    );
}
