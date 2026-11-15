use rusqlite::params;

use super::super::super::lexical::{knowledge_bm25_search, knowledge_trigram_search};
use super::super::super::*;
use super::super::{make_store, seed_chunk};
use super::corrupt_metadata_as_blob;

#[test]
fn bm25_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn trigram_search_errors_on_undecodable_row_instead_of_dropping_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/healthy.md",
        "Whale A",
        "whale migration notes",
        "{}",
    );
    let corrupt = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/corrupt.md",
        "Whale B",
        "whale migration notes",
        "{}",
    );
    corrupt_metadata_as_blob(&store, corrupt);

    let result = knowledge_trigram_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "corrupt row must not be silently dropped");
}

#[test]
fn bm25_search_errors_on_non_integer_line_number_instead_of_zero() {
    let store = make_store();
    let rowid = seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/lines.md",
        "Whale notes",
        "whale migration notes",
        "{}",
    );
    store
        .conn()
        .execute(
            "UPDATE knowledge_chunk_meta SET line_start = 'bad' WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();

    let result = knowledge_bm25_search(store.conn(), "whale", 10, "", &[]);

    assert!(result.is_err(), "line numbers must not default to 0");
}

#[test]
fn search_errors_on_invalid_chunk_metadata_instead_of_defaulting_it() {
    let store = make_store();
    seed_chunk(
        &store,
        "repo",
        "/repo",
        "src/bad_meta.md",
        "Whale notes",
        "whale migration notes",
        "{not json",
    );

    let result = knowledge_search_report(
        &store,
        "whale migration",
        None,
        None,
        5,
        &SearchWeights::default(),
        1_000,
    );

    assert!(result.is_err(), "invalid metadata JSON must be reported");
}
