use super::super::*;
use crate::support::{add_source, make_store};
use tempfile::tempdir;

#[test]
fn enrichment_fn_metadata_stored() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.md"), "# Enriched\nContent").unwrap();
    let source = add_source(&store, "test", dir.path());

    let enrich = |_content: &str, _path: &Path| -> serde_json::Value {
        serde_json::json!({"frontmatter": {"type": "task", "status": "active"}})
    };

    sync_source(&store, &source, Some(&enrich)).unwrap();

    let metadata: String = store
        .conn()
        .query_row(
            "SELECT metadata FROM knowledge_chunk_meta WHERE source_id = ?1 LIMIT 1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(parsed["frontmatter"]["type"], "task");
    assert_eq!(parsed["frontmatter"]["status"], "active");
}

#[test]
fn walk_skips_empty_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("empty.md"), "").unwrap();
    fs::write(dir.path().join("content.md"), "has content").unwrap();

    let files = walk_source_dir(dir.path(), &[]).unwrap();
    assert_eq!(files.len(), 1);
    assert!(files.contains_key("content.md"));
}

#[test]
fn walk_errors_on_nonexistent_path() {
    let result = walk_source_dir(Path::new("/tmp/surely_nonexistent_dir_abc123"), &[]);
    assert!(result.is_err());
}

#[test]
fn sha256_produces_hex_string() {
    let hash = sha256_hex(b"hello world");
    assert_eq!(hash.len(), 64, "SHA-256 hex is 64 chars");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_is_deterministic() {
    assert_eq!(sha256_hex(b"same input"), sha256_hex(b"same input"));
}

#[test]
fn sha256_differs_for_different_input() {
    assert_ne!(sha256_hex(b"input a"), sha256_hex(b"input b"));
}
