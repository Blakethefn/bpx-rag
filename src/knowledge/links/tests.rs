use super::{merge_neighbors, RELATED_COLUMNS};
use crate::knowledge::{open_in_memory, RelatedChunk};
use rusqlite::params;
use std::collections::HashMap;

mod positions;

#[test]
fn neighbors_follow_the_top_hits_without_duplicates() {
    let ranked = vec!["a", "b", "c", "d", "e"];
    let neighbors = vec!["x", "a", "y"];
    let merged = merge_neighbors(ranked, neighbors, 2, 6, |item| item.to_string());
    assert_eq!(merged, vec!["a", "b", "x", "y", "c", "d"]);
}

#[test]
fn neighbors_never_push_out_the_top_hits() {
    let ranked = vec!["a", "b"];
    let neighbors = vec!["x", "y", "z"];
    let merged = merge_neighbors(ranked, neighbors, 2, 2, |item| item.to_string());
    assert_eq!(merged, vec!["a", "b"]);
}

#[test]
fn related_columns_are_file_path_title_content_and_lines() {
    assert_eq!(
        RELATED_COLUMNS,
        [
            "kcm.file_id",
            "kf.rel_path",
            "kc.title",
            "kc.content",
            "COALESCE(kcm.line_start, 0)",
            "COALESCE(kcm.line_end, 0)",
        ]
        .as_slice()
    );
}


fn insert_file(conn: &rusqlite::Connection, source_id: i64, rel_path: &str) -> i64 {
    conn.execute(
        "INSERT INTO knowledge_files (source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, ?2, 'hash', 1)",
        params![source_id, rel_path],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_chunk(
    conn: &rusqlite::Connection,
    source_id: i64,
    rel_path: &str,
    title: &str,
    content: &str,
    line_start: Option<i64>,
    line_end: Option<i64>,
) -> i64 {
    let file_id = insert_file(conn, source_id, rel_path);
    conn.execute(
        "INSERT INTO knowledge_chunks (title, content) VALUES (?1, ?2)",
        params![title, content],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta
            (chunk_rowid, file_id, source_id, line_start, line_end)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![rowid, file_id, source_id, line_start, line_end],
    )
    .unwrap();
    file_id
}

fn insert_link(conn: &rusqlite::Connection, source_file_id: i64, target_file_id: i64) {
    conn.execute(
        "INSERT INTO knowledge_links (source_file_id, target_file_id, link_type)
         VALUES (?1, ?2, 'wikilink')",
        params![source_file_id, target_file_id],
    )
    .unwrap();
}

fn by_path(chunks: &[RelatedChunk]) -> HashMap<&str, &RelatedChunk> {
    chunks
        .iter()
        .map(|chunk| (chunk.rel_path.as_str(), chunk))
        .collect()
}

fn assert_chunk(
    chunk: &RelatedChunk,
    file_id: i64,
    rel_path: &str,
    title: &str,
    content: &str,
    line_start: u32,
    line_end: u32,
) {
    assert_eq!(chunk.file_id, file_id);
    assert_eq!(chunk.rel_path, rel_path);
    assert_eq!(chunk.title, title);
    assert_eq!(chunk.content, content);
    assert_eq!(chunk.line_start, line_start);
    assert_eq!(chunk.line_end, line_end);
}
