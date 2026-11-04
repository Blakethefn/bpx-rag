use crate::chunk::{chunk_content_for_path, chunker_version_for_path, Symbol};
use anyhow::{Context, Result};
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fs, io};

pub(super) struct PreparedFile {
    pub(super) rel_path: String,
    pub(super) abs_path: PathBuf,
    pub(super) size_bytes: i64,
    pub(super) hash: String,
    pub(super) file_id: Option<i64>,
    pub(super) metadata: String,
    pub(super) chunks: Vec<crate::chunk::Chunk>,
    pub(super) chunker_version: i64,
    pub(super) used_tree_sitter: bool,
    pub(super) symbols: Vec<Symbol>,
    pub(super) vectors: Vec<Vec<f32>>,
}

/// Compute SHA-256 hex digest of bytes.
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Existing file record: (file id, content hash, chunker version, symbol version).
pub(super) type DbFileState = (i64, String, i64, i64);

/// Load existing file records from the knowledge DB for a source.
pub(super) fn load_db_files(
    conn: &rusqlite::Connection,
    source_id: i64,
) -> Result<HashMap<String, DbFileState>> {
    let mut stmt = conn.prepare(
        "SELECT id, rel_path, content_hash, chunker_version, symbol_version
         FROM knowledge_files WHERE source_id = ?1",
    )?;
    let mut files = HashMap::new();
    let rows = stmt.query_map(params![source_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    for row in rows {
        let (id, rel_path, hash, chunker_version, symbol_version) = row?;
        files.insert(rel_path, (id, hash, chunker_version, symbol_version));
    }
    Ok(files)
}

pub(super) fn prepare_file(
    rel_path: String,
    abs_path: PathBuf,
    content: String,
    hash: String,
    file_id: Option<i64>,
    enrichment_fn: Option<&dyn Fn(&str, &Path) -> serde_json::Value>,
) -> PreparedFile {
    let size_bytes = content.len() as i64;
    let metadata = match enrichment_fn {
        Some(f) => f(&content, &abs_path),
        None => serde_json::json!({}),
    };
    let chunking = chunk_content_for_path(&abs_path, &content);
    let chunker_version = chunker_version_for_path(Path::new(&rel_path));
    PreparedFile {
        rel_path,
        abs_path,
        size_bytes,
        hash,
        file_id,
        metadata: metadata.to_string(),
        chunks: chunking.chunks,
        chunker_version,
        used_tree_sitter: chunking.used_tree_sitter,
        symbols: chunking.symbols,
        vectors: Vec::new(),
    }
}

pub(super) fn read_classified_file(abs_path: &Path, expected_hash: &str) -> Result<String> {
    let bytes =
        fs::read(abs_path).with_context(|| format!("failed to read {}", abs_path.display()))?;
    let content = String::from_utf8(bytes)
        .with_context(|| format!("file is not valid UTF-8: {}", abs_path.display()))?;
    if sha256_hex(content.as_bytes()) != expected_hash {
        anyhow::bail!(
            "source file changed before preparation: {}; retry sync",
            abs_path.display()
        );
    }
    Ok(content)
}

/// Check if a file is binary by looking for null bytes in the first 1KB.
pub(super) fn is_binary(path: &Path) -> io::Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0u8; 1024];
    let bytes_read = file.read(&mut buf)?;
    Ok(buf[..bytes_read].contains(&0u8))
}

// ── Wikilink graph resolution ──────────────────────────────────────────────
