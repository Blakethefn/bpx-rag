use super::super::super::*;
use crate::support::{add_source, make_store};
use tempfile::tempdir;

#[test]
fn sync_works_without_embedder() {
    let store = make_store();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.md"), "# No Embed\nStill works").unwrap();

    let source = add_source(&store, "test", dir.path());
    let result = sync_source(&store, &source, None).unwrap();

    assert_eq!(result.files_added, 1);
    let embed_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM knowledge_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(embed_count, 0, "no embeddings without embedder");
}
