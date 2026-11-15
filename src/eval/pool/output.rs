use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn opaque_id(seed: u64, kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_be_bytes());
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{kind}-{}", &digest[..20])
}

pub(super) fn normalized_output_target(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("output path must include a file name"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        anyhow::bail!("output directory does not exist: {}", parent.display());
    }
    Ok(parent.canonicalize()?.join(file_name))
}

pub(super) fn ensure_new_output(path: &Path) -> Result<()> {
    let _ = normalized_output_target(path)?;
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing output {}", path.display());
    }
    Ok(())
}

pub(super) fn ensure_new_distinct_outputs(first: &Path, second: &Path) -> Result<()> {
    ensure_new_output(first)?;
    ensure_new_output(second)?;
    if normalized_output_target(first)? == normalized_output_target(second)? {
        anyhow::bail!("pool and reveal-key outputs must be different files");
    }
    Ok(())
}

pub(super) fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

pub(super) fn write_new_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("could not create output {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

pub(super) fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON file {}", path.display()))
}
