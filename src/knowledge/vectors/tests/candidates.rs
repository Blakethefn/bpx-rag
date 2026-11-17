use super::super::candidates::{global_language_rowids, global_path_rowids};
use crate::knowledge::filter::{parse_filter, predicates_to_sql};
use crate::knowledge::{open_in_memory, KnowledgeStore};
use rusqlite::{params, Connection};
use std::collections::HashSet;

fn add_source(store: &KnowledgeStore, label: &str) -> i64 {
    store
        .add_source(label, &format!("/synthetic/{label}"), &[], &[])
        .unwrap()
}

fn add_chunk(store: &KnowledgeStore, source_id: i64, path: &str, metadata: &str) -> i64 {
    let conn = store.conn();
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, extension, content_hash, size_bytes)
         VALUES (?1, ?2, ?3, 'hash', 1)",
        params![
            source_id,
            path,
            crate::knowledge::filter::file_extension(path)
        ],
    )
    .unwrap();
    let file_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES ('title', 'content')",
        [],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, metadata)
         VALUES (?1, ?2, ?3, ?4)",
        params![rowid, file_id, source_id, metadata],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
         VALUES (?1, ?2, 2, 'test-model')",
        params![rowid, crate::embed::embedding_to_bytes(&[1.0, 0.0])],
    )
    .unwrap();
    rowid
}

fn baseline_rowids(
    conn: &Connection,
    predicates: &[crate::knowledge::filter::FilterPredicate],
) -> HashSet<i64> {
    let (meta_where, meta_values) = predicates_to_sql(predicates, 1);
    let clause = if meta_where.is_empty() {
        String::new()
    } else {
        format!(" AND {meta_where}")
    };
    let sql = format!(
        "SELECT ke.chunk_rowid FROM knowledge_embeddings ke
         JOIN knowledge_chunk_meta kcm ON kcm.chunk_rowid = ke.chunk_rowid
         JOIN knowledge_files kf ON kf.id = kcm.file_id
         WHERE ke.model_id = ?1{clause}"
    );
    let params: Vec<Box<dyn rusqlite::types::ToSql>> =
        std::iter::once(Box::new("test-model".to_string()) as Box<dyn rusqlite::types::ToSql>)
            .chain(
                meta_values
                    .into_iter()
                    .map(|value| Box::new(value) as Box<dyn rusqlite::types::ToSql>),
            )
            .collect();
    let refs = params
        .iter()
        .map(|value| value.as_ref())
        .collect::<Vec<_>>();
    conn.prepare(&sql)
        .unwrap()
        .query_map(refs.as_slice(), |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn global_file_first_allowlist_matches_the_current_predicates() {
    let store = open_in_memory().unwrap();
    let repo = add_source(&store, "repo");
    let other = add_source(&store, "other");
    let in_scope = add_chunk(
        &store,
        repo,
        "src/a.rs",
        r#"{"frontmatter":{"type":"code","tags":["keep"]}}"#,
    );
    add_chunk(
        &store,
        repo,
        "src/b.rs",
        r#"{"frontmatter":{"type":"note","tags":["keep"]}}"#,
    );
    add_chunk(
        &store,
        repo,
        "docs/a.rs",
        r#"{"frontmatter":{"type":"code","tags":["keep"]}}"#,
    );
    let same_path_other_source = add_chunk(
        &store,
        other,
        "src/a.rs",
        r#"{"frontmatter":{"type":"code","tags":["keep"]}}"#,
    );

    for filter in [
        "dir:src type:code tag:keep",
        "source:repo dir:src type:code tag:keep",
        "source:repo file:src/a.rs type:code tag:keep",
        "source:repo dir:src status:active",
    ] {
        let predicates = parse_filter(Some(filter)).unwrap();
        let actual = global_path_rowids(store.conn(), "test-model", &predicates)
            .unwrap()
            .unwrap();
        let expected = baseline_rowids(store.conn(), &predicates);
        assert_eq!(actual, expected, "{filter}");
    }

    let unscoped = parse_filter(Some("dir:src type:code tag:keep")).unwrap();
    let unscoped_rows = global_path_rowids(store.conn(), "test-model", &unscoped)
        .unwrap()
        .unwrap();
    assert!(unscoped_rows.contains(&same_path_other_source));
    let source_scoped = parse_filter(Some("source:repo dir:src type:code tag:keep")).unwrap();
    let source_rows = global_path_rowids(store.conn(), "test-model", &source_scoped)
        .unwrap()
        .unwrap();
    assert_eq!(source_rows, HashSet::from([in_scope]));
}

#[test]
fn global_language_allowlist_matches_existing_filter_semantics() {
    let store = open_in_memory().unwrap();
    let repo = add_source(&store, "repo");
    let markdown = add_chunk(
        &store,
        repo,
        "docs/README.MD",
        r#"{"frontmatter":{"type":"guide","tags":["keep"]}}"#,
    );
    let mdx = add_chunk(&store, repo, "docs/page.mdx", r#"{"frontmatter":{}}"#);
    let frontmatter_alias = add_chunk(
        &store,
        repo,
        "notes/no-extension",
        r#"{"frontmatter":{"language":"MD","type":"guide","tags":["keep"]}}"#,
    );
    let makefile = add_chunk(&store, repo, "tools/GNUMakefile", r#"{"frontmatter":{}}"#);
    let c_header = add_chunk(&store, repo, "src/shared.h", r#"{"frontmatter":{}}"#);
    let cpp_header = add_chunk(&store, repo, "src/cpp.hpp", r#"{"frontmatter":{}}"#);
    let rust = add_chunk(&store, repo, "src/lib.rs", r#"{"frontmatter":{}}"#);
    let unknown = add_chunk(&store, repo, "data/custom.xyz", r#"{"frontmatter":{}}"#);
    let markdown_dotfile = add_chunk(&store, repo, ".md", r#"{"frontmatter":{}}"#);
    let rust_dotfile = add_chunk(&store, repo, "src/.rs", r#"{"frontmatter":{}}"#);

    for (filter, expected) in [
        (
            "language:markdown",
            HashSet::from([markdown, mdx, frontmatter_alias, markdown_dotfile]),
        ),
        (
            "language:md",
            HashSet::from([markdown, mdx, frontmatter_alias, markdown_dotfile]),
        ),
        ("language:make", HashSet::from([makefile])),
        ("language:c", HashSet::from([c_header])),
        ("language:cpp", HashSet::from([c_header, cpp_header])),
        ("language:rs", HashSet::from([rust, rust_dotfile])),
        ("language:xyz", HashSet::from([unknown])),
        (
            "language:markdown type:guide tag:keep",
            HashSet::from([markdown, frontmatter_alias]),
        ),
        (
            "source:repo dir:docs language:markdown type:guide tag:keep",
            HashSet::from([markdown]),
        ),
    ] {
        let predicates = parse_filter(Some(filter)).unwrap();
        let actual = global_language_rowids(store.conn(), "test-model", &predicates)
            .unwrap()
            .unwrap();
        assert_eq!(actual, expected, "{filter}");
        assert_eq!(
            actual,
            baseline_rowids(store.conn(), &predicates),
            "{filter}"
        );
    }
}
