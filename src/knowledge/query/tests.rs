use super::*;

mod diversity;
mod embedding;
mod layer;
mod retrieval;
mod eval;
mod sanitize;
mod result;
mod filter;
mod rows;
mod vector;
mod fuzzy;
mod vocab;
mod identifier;
mod rules;

use crate::knowledge;
use rusqlite::params;

pub(super) fn make_store() -> KnowledgeStore {
    knowledge::open_in_memory().expect("in-memory knowledge store")
}

/// Insert a source + file + chunk with metadata for testing search.
pub(super) fn seed_chunk(
    store: &KnowledgeStore,
    source_label: &str,
    source_path: &str,
    rel_path: &str,
    title: &str,
    content: &str,
    metadata_json: &str,
) -> i64 {
    let conn = store.conn();

    // Ensure source exists
    let source_id: i64 = match conn.query_row(
        "SELECT id FROM knowledge_sources WHERE label = ?1",
        params![source_label],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO knowledge_sources (label, path, enrichments) VALUES (?1, ?2, '[]')",
                params![source_label, source_path],
            )
            .unwrap();
            conn.last_insert_rowid()
        }
    };

    // Ensure file exists
    let file_id: i64 = match conn.query_row(
        "SELECT id FROM knowledge_files WHERE source_id = ?1 AND rel_path = ?2",
        params![source_id, rel_path],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes) VALUES (?1, ?2, 'abc123', 100)",
                params![source_id, rel_path],
            )
            .unwrap();
            conn.last_insert_rowid()
        }
    };

    // Insert into FTS5
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
        params![title, content],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();

    // Insert matching trigram row
    conn.execute(
        "INSERT INTO knowledge_chunks_trigram (rowid, title, content) VALUES (?1, ?2, ?3)",
        params![rowid, title, content],
    )
    .unwrap();

    // Insert matching identifier words, as indexing does
    conn.execute(
        "INSERT INTO knowledge_identifiers (rowid, words) VALUES (?1, ?2)",
        params![
            rowid,
            crate::knowledge::identifiers::identifier_words(&format!("{title} {content}"))
        ],
    )
    .unwrap();

    // Insert chunk_meta
    conn.execute(
        "INSERT INTO knowledge_chunk_meta (chunk_rowid, file_id, source_id, line_start, line_end, metadata) VALUES (?1, ?2, ?3, 1, 20, ?4)",
        params![rowid, file_id, source_id, metadata_json],
    )
    .unwrap();

    rowid
}

#[test]
fn fuzzy_search_runs_on_a_query_only_connection() {
    // With `query_only` set, any write fails. A search that corrects a misspelled word goes
    // through the vocabulary read, so this proves the search path performs no writes.
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/a.md",
        "Whale notes",
        "whale migration routes",
        "{}",
    );
    store
        .conn()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();

    let report = knowledge_search_report(
        &store,
        "whale migraton",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    )
    .expect("a search must not need to write");

    assert!(report
        .results
        .iter()
        .any(|result| result.file_path == "src/a.md"));
}
