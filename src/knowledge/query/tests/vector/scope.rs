use super::{knowledge_vector_search, make_store, seed_chunk, seed_vector};
use rusqlite::params;

fn old_model_first_search(
    store: &crate::knowledge::KnowledgeStore,
    query: &[f32],
    predicates: &[crate::knowledge::filter::FilterPredicate],
) -> Vec<(i64, f32)> {
    let (meta_where, meta_params) = crate::knowledge::filter::predicates_to_sql(predicates, 0);
    let meta_clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };
    let model_param = meta_params.len() + 1;
    let sql = format!(
        "SELECT ke.chunk_rowid, ke.embedding
         FROM knowledge_embeddings ke
         JOIN knowledge_chunks kc ON kc.rowid = ke.chunk_rowid
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE 1=1{meta_clause} AND ke.model_id = ?{model_param}"
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = meta_params
        .into_iter()
        .map(|value| Box::new(value) as _)
        .collect();
    params.push(Box::new("test-model".to_string()));
    let refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|value| value.as_ref()).collect();
    let rows = store
        .conn()
        .prepare(&sql)
        .unwrap()
        .query_map(refs.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut scored = rows
        .into_iter()
        .map(|(rowid, bytes)| {
            let vector = crate::embed::bytes_to_embedding(&bytes);
            let score = query
                .iter()
                .zip(vector.iter())
                .fold(0.0_f32, |sum, (left, right)| sum + left * right);
            (rowid, score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(10);
    scored
}

fn candidate_search(
    store: &crate::knowledge::KnowledgeStore,
    predicates: &[crate::knowledge::filter::FilterPredicate],
) -> Vec<(i64, f32)> {
    let query = [1.0, 0.0];
    knowledge_vector_search(store.conn(), &query, 10, predicates, "test-model", None)
        .unwrap()
        .into_iter()
        .map(|result| {
            let bytes: Vec<u8> = store
                .conn()
                .query_row(
                    "SELECT embedding FROM knowledge_embeddings WHERE chunk_rowid = ?1",
                    params![result.rowid],
                    |row| row.get(0),
                )
                .unwrap();
            let vector = crate::embed::bytes_to_embedding(&bytes);
            let score = query
                .iter()
                .zip(vector.iter())
                .fold(0.0_f32, |sum, (left, right)| sum + left * right);
            (result.rowid, score)
        })
        .collect()
}

#[test]
fn source_path_fallback_and_global_dir_file_scopes_match_prechange_ids_and_scores() {
    let store = make_store();
    let first = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/z-last.rs",
        "Code",
        "code source",
        r#"{"frontmatter":{"type":"code"}}"#,
    );
    let second = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/a-first.rs",
        "Code",
        "code source",
        r#"{"frontmatter":{"type":"code"}}"#,
    );
    let note = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/note.md",
        "Note",
        "code source",
        r#"{"frontmatter":{"type":"note"}}"#,
    );
    let other = seed_chunk(
        &store,
        "other",
        "/other",
        "src/a-first.rs",
        "Other source",
        "code source",
        r#"{"frontmatter":{"type":"code"}}"#,
    );
    seed_vector(&store, first, &[1.0, 0.0]);
    seed_vector(&store, second, &[1.0, 0.0]);
    seed_vector(&store, note, &[0.9, 0.1]);
    seed_vector(&store, other, &[1.0, 0.0]);

    for filter in ["dir:src type:code", "source:repo dir:src type:code"] {
        let predicates = crate::knowledge::filter::scope_predicates(Some(filter), None).unwrap();
        assert_eq!(
            candidate_search(&store, &predicates),
            old_model_first_search(&store, &[1.0, 0.0], &predicates),
            "{filter}"
        );
    }

    let file_scope = crate::knowledge::filter::scope_predicates(
        Some("source:repo file:src/z-last.rs type:code"),
        None,
    )
    .unwrap();
    assert_eq!(
        candidate_search(&store, &file_scope),
        old_model_first_search(&store, &[1.0, 0.0], &file_scope)
    );

    let global_file =
        crate::knowledge::filter::scope_predicates(Some("file:src/a-first.rs type:code"), None)
            .unwrap();
    assert_eq!(
        candidate_search(&store, &global_file),
        old_model_first_search(&store, &[1.0, 0.0], &global_file)
    );
}
