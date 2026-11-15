use super::super::*;
use crate::knowledge::{KnowledgeStore, open_in_memory};
use anyhow::Result;
use rusqlite::params;

fn seed_file(store: &KnowledgeStore, source: &str, path: &str, kind: &str) -> Result<()> {
    let source_id = match store.get_source(source)? {
        Some(source) => source.id,
        None => store.add_source(source, "/synthetic", &[], &[])?,
    };
    store.conn().execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, ?2, 'hash', 1)",
        params![source_id, path],
    )?;
    let file_id = store.conn().last_insert_rowid();
    store.conn().execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('title', 'content')",
        [],
    )?;
    let chunk_rowid = store.conn().last_insert_rowid();
    store.conn().execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, metadata)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            chunk_rowid,
            file_id,
            source_id,
            format!(r#"{{"frontmatter":{{"type":"{kind}"}}}}"#)
        ],
    )?;
    Ok(())
}

fn matches(store: &KnowledgeStore, filter: &str) -> Result<Vec<(String, String)>> {
    let predicates = parse_filter(Some(filter))?;
    let (where_clause, values) = predicates_to_sql(&predicates, 0);
    let sql = format!(
        "SELECT ks.label, kf.rel_path
         FROM knowledge_chunk_meta kcm
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         JOIN knowledge_sources ks ON ks.id = kcm.source_id
         WHERE {where_clause} ORDER BY ks.label, kf.rel_path"
    );
    let mut statement = store.conn().prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(values.iter()), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[test]
fn directory_file_and_other_filters_compose_without_leaking_scope() -> Result<()> {
    let store = open_in_memory()?;
    seed_file(&store, "first", "src/foo/a.rs", "task")?;
    seed_file(&store, "first", "src/foobar/a.rs", "task")?;
    seed_file(&store, "second", "src/foo/b.rs", "task")?;
    seed_file(&store, "first", "src/%_*/literal.rs", "task")?;
    seed_file(&store, "first", "src/%_x/literal.rs", "task")?;

    assert_eq!(
        matches(&store, "dir:src/foo")?,
        vec![
            ("first".to_string(), "src/foo/a.rs".to_string()),
            ("second".to_string(), "src/foo/b.rs".to_string()),
        ]
    );
    assert_eq!(
        matches(&store, "source:first dir:src/foo type:task")?,
        vec![("first".to_string(), "src/foo/a.rs".to_string())]
    );
    assert_eq!(
        matches(&store, "file:src/%_*/literal.rs")?,
        vec![("first".to_string(), "src/%_*/literal.rs".to_string())]
    );
    assert_eq!(
        matches(&store, "dir:src/%_*")?,
        vec![("first".to_string(), "src/%_*/literal.rs".to_string())]
    );
    Ok(())
}
