use super::*;

const BM25_SELECT: &str = "kc.rowid, kcm.file_id, kcm.source_id, kc.title, kc.content, COALESCE(kcm.line_start, 0), COALESCE(kcm.line_end, 0), COALESCE(kcm.metadata, '{}'), COALESCE(ks.label, '') as source_label, COALESCE(kf.rel_path, '') as file_path, bm25(knowledge_chunks) as rank";

#[test]
fn lexical_selects_share_the_chunk_columns_and_append_rank() {
    let bm25 = lexical_columns("kc", "bm25(knowledge_chunks)");
    let trigram = lexical_columns("kct", "bm25(knowledge_chunks_trigram)");
    let identifier = lexical_columns("kc", "bm25(knowledge_identifiers)");

    assert_eq!(select_list(&bm25), BM25_SELECT);
    assert_eq!(
        select_list(&trigram),
        BM25_SELECT
            .replace("kc.", "kct.")
            .replace("bm25(knowledge_chunks)", "bm25(knowledge_chunks_trigram)")
    );
    assert_eq!(
        select_list(&identifier),
        BM25_SELECT.replace("bm25(knowledge_chunks)", "bm25(knowledge_identifiers)")
    );

    let shared: Vec<Selected> = CHUNK_COLUMNS
        .into_iter()
        .map(Selected::Chunk)
        .chain(std::iter::once(Selected::Rank))
        .collect();
    for columns in [&bm25, &trigram, &identifier] {
        let selected: Vec<Selected> = columns.iter().map(|column| column.selected).collect();
        assert_eq!(selected, shared);
    }
}
