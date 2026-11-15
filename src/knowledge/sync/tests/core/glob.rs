use super::super::super::*;
use super::super::register_source_glob;
use crate::support::make_store;
use tempfile::tempdir;

#[test]
fn sync_respects_glob_filter() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("notes.md"), "# Notes\nContent").unwrap();
    fs::write(dir.path().join("readme.txt"), "Some text").unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}").unwrap();

    let source = register_source_glob(&store, dir.path(), "*.md");
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 1, "only .md files should match");
}
