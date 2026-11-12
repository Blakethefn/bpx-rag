use super::super::*;
use super::{chunk_count_for_source, file_count_for_source, register_source_glob};
use crate::support::{add_source, make_store};
use tempfile::tempdir;

// ── sync_source ─────────────────────────────────────────────────────────

#[test]
fn sync_new_source_indexes_all_files() {
    let store = make_store();
    let dir = tempdir().unwrap();
    for i in 0..5 {
        fs::write(
            dir.path().join(format!("file{i}.md")),
            format!("# File {i}\nContent for file {i}"),
        )
        .unwrap();
    }
    let source = add_source(&store, "test", dir.path());
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 5);
    assert_eq!(result.files_unchanged, 0);
    assert_eq!(result.files_updated, 0);
    assert_eq!(result.files_removed, 0);
    assert!(result.chunks_total > 0, "should produce chunks");
    assert_eq!(file_count_for_source(&store, source.id), 5);
}

#[test]
fn sync_assigns_file_ids_in_path_order() {
    // Rowids break ties in search results, so insert order must not depend on
    // the directory walk or on hash-map iteration.
    let store = make_store();
    let dir = tempdir().unwrap();
    for name in ["zeta.md", "alpha.md", "mid.md", "beta.md"] {
        fs::write(dir.path().join(name), format!("content of {name}")).unwrap();
    }
    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();

    let mut stmt = store
        .conn()
        .prepare("SELECT rel_path FROM knowledge_files WHERE source_id = ?1 ORDER BY id")
        .unwrap();
    let ordered: Vec<String> = stmt
        .query_map(params![source.id], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(ordered, ["alpha.md", "beta.md", "mid.md", "zeta.md"]);
}

#[test]
fn sync_unchanged_reports_zero_changes() {
    let store = make_store();
    let dir = tempdir().unwrap();
    for i in 0..5 {
        fs::write(dir.path().join(format!("f{i}.md")), format!("# F{i}\nBody")).unwrap();
    }
    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_unchanged, 5);
    assert_eq!(result.files_added, 0);
    assert_eq!(result.files_updated, 0);
    assert_eq!(result.files_removed, 0);
}

#[test]
fn sync_detects_add_update_remove() {
    let store = make_store();
    let dir = tempdir().unwrap();
    for i in 0..5 {
        fs::write(
            dir.path().join(format!("f{i}.md")),
            format!("# F{i}\nOriginal"),
        )
        .unwrap();
    }
    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();

    // Modify 2, delete 1, add 1
    fs::write(dir.path().join("f0.md"), "# F0\nModified content").unwrap();
    fs::write(dir.path().join("f1.md"), "# F1\nAlso modified").unwrap();
    fs::remove_file(dir.path().join("f4.md")).unwrap();
    fs::write(dir.path().join("f5.md"), "# F5\nBrand new").unwrap();

    let source = store.get_source("test").unwrap().unwrap();
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 1);
    assert_eq!(result.files_updated, 2);
    assert_eq!(result.files_removed, 1);
    assert_eq!(result.files_unchanged, 2);
    assert_eq!(file_count_for_source(&store, source.id), 5); // 5-1+1 = 5
}

#[test]
fn sync_empty_directory() {
    let store = make_store();
    let dir = tempdir().unwrap();
    let source = add_source(&store, "test", dir.path());
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_scanned, 0);
    assert_eq!(result.files_added, 0);
    assert_eq!(result.chunks_total, 0);
}

#[test]
fn sync_skips_binary_files() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("text.md"), "# Hello\nText").unwrap();
    fs::write(dir.path().join("binary.dat"), b"data\x00\x00\x01").unwrap();

    let source = add_source(&store, "test", dir.path());
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 1, "only text file should be indexed");
}

#[test]
fn sync_invalid_utf8_skips_the_file_and_removes_its_stale_rows() {
    // Invalid UTF-8 is a reported skip. The other files still sync, and the file's old rows go
    // so the index never serves content the file no longer has.
    let store = make_store();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("keep.md");
    fs::write(&file_path, "# Kept content\nsearchable phrase").unwrap();
    fs::write(dir.path().join("other.md"), "# Other\nunrelated text").unwrap();

    let source = add_source(&store, "test", dir.path());
    sync_source(&store, &source, None).unwrap();
    assert_eq!(file_count_for_source(&store, source.id), 2);

    fs::write(&file_path, [0xff, 0xfe, 0xfd]).unwrap();
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.skipped_files.len(), 1);
    assert_eq!(result.skipped_files[0].rel_path, "keep.md");
    assert_eq!(file_count_for_source(&store, source.id), 1);
    assert!(chunk_count_for_source(&store, source.id) > 0);
}

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
