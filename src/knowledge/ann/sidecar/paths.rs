use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn model_hash(model_id: &str) -> String {
    format!("{:x}", Sha256::digest(model_id.as_bytes()))
}

pub(super) fn manifest_path(dir: &Path, model_id: &str, source_id: Option<i64>) -> Result<PathBuf> {
    let source = source_id.map_or_else(String::new, |id| format!("-source-{id}"));
    Ok(dir.join(format!("{}{source}.manifest.json", model_hash(model_id))))
}

pub(super) fn index_path(
    dir: &Path,
    model_id: &str,
    database_id: &str,
    generation: i64,
    source_id: Option<i64>,
) -> Result<PathBuf> {
    let source = source_id.map_or_else(String::new, |id| format!("source-{id}-"));
    Ok(dir.join(format!(
        "{}-{source}{}-{}.usearch",
        model_hash(model_id),
        &database_id[..database_id.len().min(12)],
        generation
    )))
}

pub(super) fn sidecar_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("index");
    path.with_file_name(format!("{name}.tmp-{}", std::process::id()))
}

pub(super) fn remove_old_sidecars(
    dir: &Path,
    model_id: &str,
    source_id: Option<i64>,
    current: &Path,
) -> Result<()> {
    let prefix = model_hash(model_id);
    let source_prefix = source_id.map(|id| format!("{prefix}-source-{id}-"));
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == current {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let same_scope = match &source_prefix {
            Some(source_prefix) => name.starts_with(source_prefix),
            None => {
                name.starts_with(&format!("{prefix}-"))
                    && !name.starts_with(&format!("{prefix}-source-"))
            }
        };
        if same_scope && name.ends_with(".usearch") {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!(
                    "[bpx-rag] could not remove stale ANN sidecar {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

pub(super) fn remove_source_sidecars(dir: &Path, source_id: i64) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let index_marker = format!("-source-{source_id}-");
    let manifest_suffix = format!("-source-{source_id}.manifest.json");
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.contains(&index_marker) || name.ends_with(&manifest_suffix) {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!(
                    "[bpx-rag] could not remove source ANN sidecar {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}
