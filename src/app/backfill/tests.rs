use super::*;
use crate::config::Config;
use crate::knowledge::KnowledgeStore;
use crate::support::add_synced_source;
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

mod batches;
mod conflict;
mod scope;

struct Fixture {
    app: RagApp,
    db_path: PathBuf,
    _data_home: TempDir,
}

impl Fixture {
    fn new(batch_size: usize) -> Self {
        let data_home = tempdir().unwrap();
        let db_path = data_home.path().join("test-knowledge.db");
        let mut config = Config::default();
        config.embeddings.batch_size = batch_size;
        let app = RagApp {
            config,
            knowledge: KnowledgeStore::open_at(&db_path).unwrap(),
            embedding_notice: None,
            embedder_loaded: true,
        };
        Self {
            app,
            db_path,
            _data_home: data_home,
        }
    }

    fn conn(&self) -> &Connection {
        self.app.knowledge.conn()
    }
}

fn write_files(dir: &Path, count: usize) {
    for index in 0..count {
        fs::write(
            dir.join(format!("file{index}.rs")),
            format!("pub fn item{index}() -> u8 {{ {index} }}\n"),
        )
        .unwrap();
    }
}

fn count_rows(conn: &Connection, sql: &str, source_id: Option<i64>) -> usize {
    let count: i64 = match source_id {
        Some(source_id) => conn.query_row(sql, params![source_id], |row| row.get(0)),
        None => conn.query_row(sql, [], |row| row.get(0)),
    }
    .unwrap();
    count as usize
}

fn chunk_count(conn: &Connection) -> usize {
    count_rows(conn, "SELECT COUNT(*) FROM knowledge_chunks", None)
}

fn embedding_count(conn: &Connection) -> usize {
    count_rows(conn, "SELECT COUNT(*) FROM knowledge_embeddings", None)
}

fn source_chunk_count(conn: &Connection, source_id: i64) -> usize {
    count_rows(
        conn,
        "SELECT COUNT(*) FROM knowledge_chunk_meta WHERE source_id = ?1",
        Some(source_id),
    )
}

fn source_embedding_count(conn: &Connection, source_id: i64) -> usize {
    count_rows(
        conn,
        "SELECT COUNT(*) FROM knowledge_embeddings e
         JOIN knowledge_chunk_meta m ON m.chunk_rowid = e.chunk_rowid
         WHERE m.source_id = ?1",
        Some(source_id),
    )
}

fn report(
    source_label: Option<&str>,
    embedded: usize,
    skipped: usize,
    remaining: usize,
) -> EmbeddingBackfillReport {
    EmbeddingBackfillReport {
        source_label: source_label.map(str::to_string),
        embedded_chunks: embedded,
        skipped_chunks: skipped,
        remaining_chunks: remaining,
    }
}
