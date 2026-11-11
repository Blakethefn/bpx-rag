use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use usearch::{Index, MetricKind, ScalarKind};

const SIDECAR_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Manifest {
    version: u32,
    database_id: String,
    model_id: String,
    dimensions: usize,
    pub(super) generation: i64,
    pub(super) tombstones: usize,
}

pub(super) fn load_sidecar(
    dir: &Path,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
) -> Result<Option<(Index, Manifest)>> {
    let manifest_path = manifest_path(dir, model_id)?;
    let Ok(manifest_bytes) = fs::read(&manifest_path) else {
        return Ok(None);
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&manifest_bytes) else {
        return Ok(None);
    };
    if manifest.version != SIDECAR_VERSION
        || manifest.model_id != model_id
        || manifest.dimensions != dimensions
        || manifest.generation > generation
        || manifest.database_id != database_id
    {
        return Ok(None);
    }
    let index_path = index_path(dir, model_id, database_id, manifest.generation)?;
    if !index_path.is_file() {
        return Ok(None);
    }
    match Index::restore(&index_path.to_string_lossy()) {
        Ok(index)
            if index.dimensions() == dimensions
                && index.metric_kind() == MetricKind::Cos
                && index.scalar_kind() == ScalarKind::F32 =>
        {
            Ok(Some((index, manifest)))
        }
        Ok(_) => Ok(None),
        Err(error) => {
            eprintln!("[bpx-rag] ignoring invalid HNSW sidecar: {error}");
            Ok(None)
        }
    }
}

pub(super) fn save_sidecar(
    dir: &Path,
    index: &Index,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
    tombstones: usize,
) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create ANN sidecar directory {}", dir.display()))?;
    let path = index_path(dir, model_id, database_id, generation)?;
    let temporary_path = sidecar_temp_path(&path);
    index
        .save(&temporary_path.to_string_lossy())
        .map_err(|error| anyhow!("failed to save HNSW sidecar: {error}"))?;
    fs::rename(&temporary_path, &path)
        .with_context(|| format!("failed to install ANN sidecar {}", path.display()))?;

    let manifest = Manifest {
        version: SIDECAR_VERSION,
        database_id: database_id.to_string(),
        model_id: model_id.to_string(),
        dimensions,
        generation,
        tombstones,
    };
    let manifest_path = manifest_path(dir, model_id)?;
    let temporary_manifest_path = sidecar_temp_path(&manifest_path);
    fs::write(&temporary_manifest_path, serde_json::to_vec(&manifest)?)?;
    fs::rename(&temporary_manifest_path, &manifest_path).with_context(|| {
        format!(
            "failed to install ANN sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    remove_old_sidecars(dir, model_id, &path)?;
    Ok(())
}

fn model_hash(model_id: &str) -> String {
    format!("{:x}", Sha256::digest(model_id.as_bytes()))
}

fn manifest_path(dir: &Path, model_id: &str) -> Result<PathBuf> {
    Ok(dir.join(format!("{}.manifest.json", model_hash(model_id))))
}

fn index_path(dir: &Path, model_id: &str, database_id: &str, generation: i64) -> Result<PathBuf> {
    Ok(dir.join(format!(
        "{}-{}-{}.usearch",
        model_hash(model_id),
        &database_id[..database_id.len().min(12)],
        generation
    )))
}

fn sidecar_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("index");
    path.with_file_name(format!("{name}.tmp-{}", std::process::id()))
}

fn remove_old_sidecars(dir: &Path, model_id: &str, current: &Path) -> Result<()> {
    let prefix = model_hash(model_id);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == current {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".usearch") {
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
