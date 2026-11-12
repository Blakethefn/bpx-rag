use super::super::lexical::{chunk_columns, column_at, select_list, ChunkColumn, Selected};
use super::vector_columns;

#[test]
fn vector_select_inserts_embedding_after_rowid_in_the_shared_list() {
    let shared = chunk_columns("kc");
    let columns = vector_columns("kc");
    let rowid_at = column_at(&columns, Selected::Chunk(ChunkColumn::Rowid));

    let mut without_embedding = columns.clone();
    let embedding = without_embedding.remove(rowid_at + 1);
    assert_eq!(embedding.selected, Selected::Embedding);
    assert_eq!(embedding.sql, "ke.embedding");
    assert_eq!(without_embedding, shared);
    assert_eq!(
        select_list(&columns),
        "kc.rowid, ke.embedding, kcm.file_id, kcm.source_id, kc.title, kc.content, COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0), COALESCE(kcm.metadata, '{}'), COALESCE(ks.label, '') as source_label, COALESCE(kf.rel_path, '') as file_path"
    );
}
