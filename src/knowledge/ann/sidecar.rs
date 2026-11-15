use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use usearch::{Index, MetricKind, ScalarKind};

mod paths;

use paths::{
    index_path, manifest_path, remove_old_sidecars, remove_source_sidecars, sidecar_temp_path,
};

const SIDECAR_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Manifest {
    version: u32,
    database_id: String,
    model_id: String,
    dimensions: usize,
    #[serde(default)]
    source_id: Option<i64>,
    pub(super) generation: i64,
    pub(super) tombstones: usize,
}

#[cfg(test)]
pub(super) fn load_sidecar(
    dir: &Path,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
) -> Result<Option<(Index, Manifest)>> {
    load_sidecar_scoped(dir, model_id, dimensions, generation, database_id, None)
}

pub(super) fn load_sidecar_scoped(
    dir: &Path,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
    source_id: Option<i64>,
) -> Result<Option<(Index, Manifest)>> {
    let manifest_path = manifest_path(dir, model_id, source_id)?;
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
        || manifest.source_id != source_id
    {
        return Ok(None);
    }
    let index_path = index_path(dir, model_id, database_id, manifest.generation, source_id)?;
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

#[cfg(test)]
pub(super) fn save_sidecar(
    dir: &Path,
    index: &Index,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
    tombstones: usize,
) -> Result<()> {
    save_sidecar_scoped(
        dir,
        index,
        model_id,
        dimensions,
        generation,
        database_id,
        tombstones,
        None,
    )
}

pub(super) fn save_sidecar_scoped(
    dir: &Path,
    index: &Index,
    model_id: &str,
    dimensions: usize,
    generation: i64,
    database_id: &str,
    tombstones: usize,
    source_id: Option<i64>,
) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create ANN sidecar directory {}", dir.display()))?;
    let path = index_path(dir, model_id, database_id, generation, source_id)?;
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
        source_id,
        generation,
        tombstones,
    };
    let manifest_path = manifest_path(dir, model_id, source_id)?;
    let temporary_manifest_path = sidecar_temp_path(&manifest_path);
    fs::write(&temporary_manifest_path, serde_json::to_vec(&manifest)?)?;
    fs::rename(&temporary_manifest_path, &manifest_path).with_context(|| {
        format!(
            "failed to install ANN sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    remove_old_sidecars(dir, model_id, source_id, &path)?;
    Ok(())
}

pub(super) fn remove_source(dir: &Path, source_id: i64) -> Result<()> {
    remove_source_sidecars(dir, source_id)
}
