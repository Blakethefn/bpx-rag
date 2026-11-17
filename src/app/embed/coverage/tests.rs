use super::*;
use crate::config::Config;
use crate::embed::{Embed, tests::MockEmbedder};
use crate::knowledge::filter::scope_predicates;
use crate::knowledge::open_in_memory;
use rusqlite::params;
use std::sync::Arc;

fn add_chunk(app: &RagApp, source_id: i64, path: &str, embedded: bool) {
    let conn = app.knowledge.conn();
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, ?2, 'hash', 1)",
        params![source_id, path],
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
         VALUES (?1, ?2, ?3, '{}')",
        params![rowid, file_id, source_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE knowledge_files SET chunk_count = chunk_count + 1 WHERE id = ?1",
        params![file_id],
    )
    .unwrap();
    if embedded {
        let vector = vec![1.0; MockEmbedder::new().dim()];
        conn.execute(
            "INSERT INTO knowledge_embeddings (chunk_rowid, embedding, dim, model_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                rowid,
                crate::embed::embedding_to_bytes(&vector),
                vector.len() as i64,
                "sentence-transformers/all-MiniLM-L6-v2"
            ],
        )
        .unwrap();
    }
}

#[test]
fn coverage_tracks_embedded_and_missing_vectors_within_path_scope() {
    let mut knowledge = open_in_memory().unwrap();
    knowledge.set_embedder(Arc::new(MockEmbedder::new()));
    let source_id = knowledge
        .add_source("repo", "/synthetic/repo", &[], &[])
        .unwrap();
    let app = RagApp {
        config: Config::default(),
        knowledge,
        embedding_notice: None,
        embedder_loaded: true,
    };
    add_chunk(&app, source_id, "docs/embedded.md", true);
    add_chunk(&app, source_id, "docs/missing.md", false);
    add_chunk(&app, source_id, "other/embedded.md", true);

    let predicates = scope_predicates(Some("source:repo dir:docs"), None).unwrap();
    let coverage = app.embedding_coverage(&predicates).unwrap().unwrap();
    assert_eq!((coverage.total, coverage.current_model), (2, 1));
    assert!(coverage.notice(true).is_some());

    app.knowledge
        .conn()
        .execute(
            "DELETE FROM knowledge_files WHERE source_id = ?1 AND rel_path = ?2",
            params![source_id, "docs/missing.md"],
        )
        .unwrap();
    let coverage = app.embedding_coverage(&predicates).unwrap().unwrap();
    assert_eq!((coverage.total, coverage.current_model), (1, 1));
    assert_eq!(coverage.notice(true), None);
}

#[test]
fn language_coverage_counts_paths_and_frontmatter_within_the_same_scope() {
    let mut knowledge = open_in_memory().unwrap();
    knowledge.set_embedder(Arc::new(MockEmbedder::new()));
    let source_id = knowledge
        .add_source("repo", "/synthetic/repo", &[], &[])
        .unwrap();
    let other_source = knowledge
        .add_source("other", "/synthetic/other", &[], &[])
        .unwrap();
    let app = RagApp {
        config: Config::default(),
        knowledge,
        embedding_notice: None,
        embedder_loaded: true,
    };
    add_chunk(&app, source_id, "docs/extension.MD", true);
    add_chunk(&app, source_id, "docs/frontmatter.txt", false);
    add_chunk(&app, source_id, "docs/custom.txt", false);
    add_chunk(&app, source_id, "outside/path.md", true);
    add_chunk(&app, other_source, "docs/foreign.md", true);
    app.knowledge
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET metadata = ?1 WHERE file_id = (
                SELECT id FROM knowledge_files WHERE rel_path = ?2
             )",
            params![
                r#"{"frontmatter":{"language":"Markdown"}}"#,
                "docs/frontmatter.txt"
            ],
        )
        .unwrap();
    app.knowledge
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET metadata = ?1 WHERE file_id = (
                SELECT id FROM knowledge_files WHERE rel_path = ?2
             )",
            params![
                r#"{"frontmatter":{"language":"python-language!"}}"#,
                "docs/custom.txt"
            ],
        )
        .unwrap();

    let predicates = scope_predicates(Some("source:repo dir:docs language:md"), None).unwrap();
    let coverage = app.embedding_coverage(&predicates).unwrap().unwrap();
    assert_eq!((coverage.total, coverage.current_model), (2, 1));

    let custom =
        scope_predicates(Some("source:repo dir:docs language:python-language!"), None).unwrap();
    let coverage = app.embedding_coverage(&custom).unwrap().unwrap();
    assert_eq!((coverage.total, coverage.current_model), (1, 0));
}

#[test]
fn language_coverage_keeps_makefile_shared_header_and_unknown_extension_matches() {
    let mut knowledge = open_in_memory().unwrap();
    knowledge.set_embedder(Arc::new(MockEmbedder::new()));
    let source_id = knowledge
        .add_source("repo", "/synthetic/repo", &[], &[])
        .unwrap();
    let app = RagApp {
        config: Config::default(),
        knowledge,
        embedding_notice: None,
        embedder_loaded: true,
    };
    add_chunk(&app, source_id, "Makefile", true);
    add_chunk(&app, source_id, "nested/GNUMakefile", false);
    add_chunk(&app, source_id, "include/shared.h", true);
    add_chunk(&app, source_id, "scripts/config.nix", true);

    for (language, expected) in [
        ("make", (2, 1)),
        ("c", (1, 1)),
        ("cpp", (1, 1)),
        ("nix", (1, 1)),
    ] {
        let filter = format!("source:repo language:{language}");
        let predicates = scope_predicates(Some(&filter), None).unwrap();
        let coverage = app.embedding_coverage(&predicates).unwrap().unwrap();
        assert_eq!(
            (coverage.total, coverage.current_model),
            expected,
            "{language}"
        );
    }
}
