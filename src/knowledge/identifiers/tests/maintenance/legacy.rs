use rusqlite::{params, Connection};

/// Mirror of `master`'s `index_prepared_file` chunk writes: content row, trigram row,
/// then metadata row. No identifier row is written.
pub(crate) fn insert_chunk(
    conn: &Connection,
    file_id: i64,
    source_id: i64,
    title: &str,
    content: &str,
) -> i64 {
    conn.execute(
        "INSERT OR IGNORE INTO knowledge_sources (id, label, path) VALUES (?1, ?2, '/unused')",
        params![source_id, format!("source-{source_id}")],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO knowledge_files (id, source_id, rel_path, content_hash, size_bytes)
         VALUES (?1, ?2, ?3, 'hash', 1)",
        params![file_id, source_id, format!("file-{file_id}.txt")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge_chunks(title, content) VALUES (?1, ?2)",
        params![title, content],
    )
    .unwrap();
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO knowledge_chunks_trigram(rowid, title, content) VALUES (?1, ?2, ?3)",
        params![rowid, title, content],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge_chunk_meta
            (chunk_rowid, file_id, source_id, line_start, line_end, metadata)
         VALUES (?1, ?2, ?3, 1, 3, '{}')",
        params![rowid, file_id, source_id],
    )
    .unwrap();
    rowid
}

/// Mirror of `master`'s `delete_file_chunks`: chunk, trigram, embedding, and metadata rows.
pub(crate) fn delete_chunks(conn: &Connection, file_id: i64) {
    let rowids: Vec<i64> = {
        let mut statement = conn
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1")
            .unwrap();
        statement
            .query_map(params![file_id], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    for rowid in &rowids {
        conn.execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![rowid],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
            params![rowid],
        )
        .unwrap();
    }
    conn.execute(
        "DELETE FROM knowledge_embeddings WHERE chunk_rowid IN
         (SELECT chunk_rowid FROM knowledge_chunk_meta WHERE file_id = ?1)",
        params![file_id],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM knowledge_chunk_meta WHERE file_id = ?1",
        params![file_id],
    )
    .unwrap();
}

/// Mirror of `master`'s `delete_file_chunks` followed by its file-row delete.
pub(crate) fn delete_file(conn: &Connection, file_id: i64) {
    delete_chunks(conn, file_id);
    conn.execute(
        "DELETE FROM knowledge_files WHERE id = ?1",
        params![file_id],
    )
    .unwrap();
}

/// Mirror of `master`'s update path for a changed file: it stores the new hash on the
/// file row, which keeps its id, and replaces its chunks.
pub(crate) fn set_content_hash(conn: &Connection, file_id: i64, hash: &str) {
    conn.execute(
        "UPDATE knowledge_files SET content_hash = ?1 WHERE id = ?2",
        params![hash, file_id],
    )
    .unwrap();
}

/// Mirror of `master`'s `remove_source` chunk deletes.
pub(crate) fn remove_source(conn: &Connection, source_id: i64) {
    let rowids: Vec<i64> = {
        let mut statement = conn
            .prepare("SELECT chunk_rowid FROM knowledge_chunk_meta WHERE source_id = ?1")
            .unwrap();
        statement
            .query_map(params![source_id], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    for rowid in &rowids {
        conn.execute(
            "DELETE FROM knowledge_chunks WHERE rowid = ?1",
            params![rowid],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM knowledge_chunks_trigram WHERE rowid = ?1",
            params![rowid],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM knowledge_embeddings WHERE chunk_rowid = ?1",
            params![rowid],
        )
        .unwrap();
    }
    conn.execute(
        "DELETE FROM knowledge_chunk_meta WHERE source_id = ?1",
        params![source_id],
    )
    .unwrap();
}
